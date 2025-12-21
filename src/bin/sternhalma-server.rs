use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use tokio::{
    net::TcpListener,
    sync::{broadcast, mpsc, oneshot},
};
use uuid::Uuid;

use axum::{
    Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::get,
};
use bytes::Bytes;
use futures::{SinkExt, StreamExt, future};
use sternhalma_server::server::{
    MainThreadMessage, Server,
    client::{Client, ClientSink, ClientStream},
    messages::{ClientMessage, ServerBroadcast, ServerMessage},
    protocol::{RemoteInMessage, RemoteOutMessage, ServerCodec},
};
use tokio_util::codec::Framed;

const LOCAL_CHANNEL_CAPACITY: usize = 32;

/// Command line arguments
#[derive(Debug, Parser)]
#[command(name = "sternhalma-server", version, about)]
struct Args {
    /// Host IP address for Raw TCP
    #[arg(long, value_name = "ADDRESS")]
    tcp: Option<String>,
    /// Host IP address for WebSocket
    #[arg(long, value_name = "ADDRESS")]
    ws: Option<String>,
    /// Maximum number of turns
    #[arg(short = 'n', long, value_name = "N")]
    max_turns: Option<usize>,
    #[arg(short, long, value_name = "SECONDS", default_value_t = 300)]
    timeout: u64,
}

/// Shared state for the application
/// This state is cloned and passed to new connections (both TCP and WebSocket)
#[derive(Clone)]
struct AppState {
    /// Channel to send messages to the main Game/Server loop
    main_tx: mpsc::Sender<MainThreadMessage>,
    /// Channel to send messages from Clients to the Server
    client_msg_tx: mpsc::Sender<ClientMessage>,
    /// Channel for the Server to broadcast messages to all Clients
    server_broadcast_tx: broadcast::Sender<ServerBroadcast>,
}

/// Handles the initial handshake with a client (both TCP and WebSocket).
///
/// This function:
/// 1. Waits for a `Hello` (new session) or `Reconnect` message.
/// 2. Contacts the main Server thread to request a player slot or validate a session.
/// 3. Sends a welcome message (or rejection) to the client.
/// 4. If successful, spawns a `Client` task to handle the connection for the duration of the game.
async fn handle_handshake(mut stream: ClientStream, mut sink: ClientSink, app_state: AppState) {
    let AppState {
        main_tx,
        client_msg_tx,
        server_broadcast_tx,
    } = app_state;

    // 1. Wait for Hello or Reconnect
    let handshake = match stream.next().await {
        Some(Ok(msg)) => msg,
        Some(Err(e)) => {
            log::error!("Failed to read handshake: {e}");
            return;
        }
        None => {
            log::error!("Connection closed during handshake");
            return;
        }
    };

    match handshake {
        RemoteInMessage::Hello => {
            // New Session - Ask Server for free player
            let (resp_tx, resp_rx) = oneshot::channel();
            if let Err(e) = main_tx
                .send(MainThreadMessage::RequestFreePlayer(resp_tx))
                .await
            {
                log::error!("Failed to contact server: {e}");
                return;
            }

            match resp_rx.await {
                Ok(Some(player)) => {
                    let session_id = Uuid::new_v4();
                    log::info!("New client assigned: {player} (Session: {session_id})");

                    // Send Welcome
                    if let Err(e) = sink.send(RemoteOutMessage::Welcome { session_id }).await {
                        log::error!("Failed to send Welcome: {e}");
                        return;
                    }

                    // Server thread -> Client thread
                    let (server_tx, server_rx) =
                        mpsc::channel::<ServerMessage>(LOCAL_CHANNEL_CAPACITY);

                    // Create client
                    match Client::new(
                        player,
                        sink,
                        stream,
                        server_rx,
                        server_broadcast_tx.subscribe(),
                        client_msg_tx,
                    ) {
                        Err(e) => log::error!("Failed to create client: {e:?}"),
                        Ok(mut client) => {
                            tokio::spawn(async move {
                                if let Err(e) = client.run().await {
                                    log::error!("Client task error: {e:?}");
                                }
                            });
                            if let Err(e) = main_tx
                                .send(MainThreadMessage::ClientConnected(
                                    player, session_id, server_tx,
                                ))
                                .await
                            {
                                log::error!("Failed to notify server of connection: {e:?}");
                            }
                        }
                    }
                }
                Ok(None) => {
                    log::warn!("No free players");
                    let _ = sink
                        .send(RemoteOutMessage::Reject {
                            reason: "Server full".to_string(),
                        })
                        .await;
                }
                Err(e) => log::error!("Server channel error: {e}"),
            }
        }
        RemoteInMessage::Reconnect { session_id: uuid } => {
            log::info!("Reconnection attempt: {uuid}");
            let (resp_tx, resp_rx) = oneshot::channel();
            if let Err(e) = main_tx
                .send(MainThreadMessage::ClientReconnectedHandle(uuid, resp_tx))
                .await
            {
                log::error!("Failed to contact server: {e}");
                return;
            }

            match resp_rx.await {
                Ok(Some(player)) => {
                    // Ack
                    if let Err(e) = sink
                        .send(RemoteOutMessage::Welcome { session_id: uuid })
                        .await
                    {
                        log::error!("Failed to send Welcome: {e}");
                        return;
                    }

                    let (server_tx, server_rx) =
                        mpsc::channel::<ServerMessage>(LOCAL_CHANNEL_CAPACITY);
                    match Client::new(
                        player,
                        sink,
                        stream,
                        server_rx,
                        server_broadcast_tx.subscribe(),
                        client_msg_tx,
                    ) {
                        Err(e) => log::error!("Failed to create client: {e:?}"),
                        Ok(mut client) => {
                            tokio::spawn(async move {
                                if let Err(e) = client.run().await {
                                    log::error!("Client task error: {e:?}");
                                }
                            });
                            let _ = main_tx
                                .send(MainThreadMessage::ClientReconnected(player, server_tx))
                                .await;
                        }
                    }
                }
                Ok(None) => {
                    log::warn!("Unknown session: {uuid}");
                    let _ = sink
                        .send(RemoteOutMessage::Reject {
                            reason: "Unknown Session".to_string(),
                        })
                        .await;
                }
                Err(e) => log::error!("Server channel error: {e}"),
            }
        }
        _ => {
            log::error!("Invalid handshake message");
        }
    };
}

/// Axum handler for WebSocket upgrades.
/// Upgrades the HTTP connection to a WebSocket connection.
async fn ws_handler(State(state): State<AppState>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_ws(socket, state))
}

/// Handles the WebSocket connection.
///
/// This function acts as an adapter, converting the WebSocket stream (of `Message::Binary`)
/// into the `RemoteInMessage` and `RemoteOutMessage` types used by the core server logic.
/// It effectively wraps the WebSocket in the same interface as the TCP connection (`ClientSink` / `ClientStream`)
/// and then delegates to `handle_handshake`.
async fn handle_ws(socket: WebSocket, state: AppState) {
    let (ws_write, ws_read) = socket.split();

    // Adapter for Stream -> RemoteInMessage
    let stream = Box::pin(ws_read.map(|msg_res| match msg_res {
        Ok(Message::Binary(bin)) => RemoteInMessage::from_bytes(&bin),
        Ok(Message::Close(_)) => Err(anyhow::anyhow!("Connection closed")),
        Ok(_) => Err(anyhow::anyhow!("Unexpected message type")),
        Err(e) => Err(anyhow::anyhow!("WS Error: {e}")),
    }));

    // Adapter for Sink -> RemoteOutMessage
    let sink = Box::pin(SinkExt::with(ws_write, |msg: RemoteOutMessage| {
        let mut buf = Vec::new();
        // Ciborium to vec
        let res = ciborium::into_writer(&msg, &mut buf)
            .map_err(|e| anyhow::anyhow!("Serialization error: {e}"))
            .map(|_| Message::Binary(Bytes::from(buf)));
        future::ready(res)
    }));

    // Reuse handle_handshake logic
    // We need to pass ClientSink/Stream. The adapters should match.
    // Wait, SinkExt::with returns a struct, I need to wrap in Box/Pin.
    // And Sink must satisfy the trait.
    // ws_write is SplitSink<WebSocket>.
    // SinkExt::with takes `self`.

    // Boxing helper
    let sink: ClientSink = Box::pin(sink);
    let stream: ClientStream = Box::pin(stream);

    handle_handshake(stream, sink, state).await;
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logger
    env_logger::init();

    // Parse command line arguments
    let args = Args::parse();
    log::debug!("Command line arguments: {args:?}");
    let timeout = Duration::from_secs(args.timeout);

    // --- Channel Setup ---
    // The server architecture relies on message passing between threads/tasks.

    // Client threads -> Server thread
    // Used for active game actions (move, disconnect)
    let (client_msg_tx, client_msg_rx) = mpsc::channel::<ClientMessage>(LOCAL_CHANNEL_CAPACITY);

    // Server thread -> Client threads
    // Used for broadcasting common information (move updates, game finish)
    let (server_broadcast_tx, _server_broadcast_rx) =
        broadcast::channel::<ServerBroadcast>(LOCAL_CHANNEL_CAPACITY);

    // Main thread -> Server thread
    // Used for connection establishment (handshake requests)
    let (main_tx, main_rx) = mpsc::channel::<MainThreadMessage>(LOCAL_CHANNEL_CAPACITY);

    // Channel for the server thread to send shutdown signal to main thread
    // If the server logic fails or finishes, it triggers a full application shutdown.
    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    // --- Spawn Game Server ---
    // The `Server` struct runs in its own task and manages the game logic.
    let server = Server::new(main_rx, client_msg_rx, server_broadcast_tx.clone())
        .with_context(|| "Failed to create server")?;

    let max_turns = args.max_turns.unwrap_or(usize::MAX);

    tokio::spawn(async move {
        if let Err(e) = server.try_run(timeout, max_turns).await {
            log::error!("Server encountered an error: {e:?}");
        }
        log::trace!("Sending shutdown signal");
        let _ = shutdown_tx.send(());
    });

    // App State held by connection handlers
    let app_state = AppState {
        main_tx: main_tx.clone(),
        client_msg_tx,
        server_broadcast_tx,
    };

    // --- Start Listener ---

    if args.tcp.is_none() && args.ws.is_none() {
        use clap::CommandFactory;
        let mut cmd = Args::command();
        cmd.error(
            clap::error::ErrorKind::MissingRequiredArgument,
            "Either --tcp or --ws must be provided",
        )
        .exit();
    }

    if let Some(addr) = args.tcp {
        // 1. TCP Listener (Raw protocol)
        let listener = TcpListener::bind(&addr)
            .await
            .with_context(|| "Failed to bind listener to socket")?;
        log::info!("Listening (TCP) at {addr}");

        let app_state = app_state.clone();
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Err(e) => {
                        log::error!("Failed to accept connection: {e:?}");
                        continue;
                    }
                    Ok((stream, _addr)) => {
                        let framed = Framed::new(stream, ServerCodec::new());
                        let (write, read) = framed.split();
                        let sink: ClientSink = Box::pin(write.sink_map_err(|e| anyhow::anyhow!(e)));
                        let stream: ClientStream =
                            Box::pin(read.map(|msg| msg.map_err(|e| anyhow::anyhow!(e))));

                        tokio::spawn(handle_handshake(stream, sink, app_state.clone()));
                    }
                }
            }
        });
    }

    if let Some(addr) = args.ws {
        // 2. WebSocket Listener (Web Clients)
        let app = Router::new()
            .route("/ws", get(ws_handler))
            .layer(tower_http::cors::CorsLayer::permissive())
            .with_state(app_state.clone());

        let listener = tokio::net::TcpListener::bind(&addr).await?;
        log::info!("Listening (WS) at {addr}");

        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                log::error!("Axum server error: {e}");
            }
        });
    }

    // Wait for shutdown signal
    shutdown_rx.await.ok();
    log::trace!("Shutdown signal received");

    Ok(())
}
