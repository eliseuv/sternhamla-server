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
use futures::{SinkExt, StreamExt};
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
    /// Host IP address
    #[arg(short, long, value_name = "ADDRESS", default_value = "127.0.0.1:8080")]
    socket: String,
    /// WebSocket port
    #[arg(long, value_name = "PORT", default_value_t = 8081)]
    ws_port: u16,
    /// Maximum number of turns
    #[arg(short = 'n', long, value_name = "N")]
    max_turns: usize,
    #[arg(short, long, value_name = "SECONDS", default_value_t = 30)]
    timeout: u64,
}

#[derive(Clone)]
struct AppState {
    main_tx: mpsc::Sender<MainThreadMessage>,
    client_msg_tx: mpsc::Sender<ClientMessage>,
    server_broadcast_tx: broadcast::Sender<ServerBroadcast>,
}

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
                    if let Err(e) = sink
                        .send(RemoteOutMessage::Welcome { session_id, player })
                        .await
                    {
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
                        .send(RemoteOutMessage::Welcome {
                            session_id: uuid,
                            player,
                        })
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

async fn ws_handler(State(state): State<AppState>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_ws(socket, state))
}

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

use futures::future;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logger
    env_logger::init();

    // Parse command line arguments
    let args = Args::parse();
    log::debug!("Command line arguments: {args:?}");
    let timeout = Duration::from_secs(args.timeout);

    // Client threads -> Server thread
    let (client_msg_tx, client_msg_rx) = mpsc::channel::<ClientMessage>(LOCAL_CHANNEL_CAPACITY);

    // Server thread -> Client threads
    let (server_broadcast_tx, _server_broadcast_rx) =
        broadcast::channel::<ServerBroadcast>(LOCAL_CHANNEL_CAPACITY);

    // Main thread -> Server thread
    let (main_tx, main_rx) = mpsc::channel::<MainThreadMessage>(LOCAL_CHANNEL_CAPACITY);

    // Channel for the server thread to send shutdown signal to main thread
    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    // Spawn the server task
    let server = Server::new(main_rx, client_msg_rx, server_broadcast_tx.clone())
        .with_context(|| "Failed to create server")?;
    tokio::spawn(async move {
        if let Err(e) = server.try_run(timeout, args.max_turns).await {
            log::error!("Server encountered an error: {e:?}");
        }
        log::trace!("Sending shutdown signal");
        let _ = shutdown_tx.send(());
    });

    // App State
    let app_state = AppState {
        main_tx: main_tx.clone(),
        client_msg_tx,
        server_broadcast_tx,
    };

    // Bind TCP listener to the socket address
    let listener = TcpListener::bind(&args.socket)
        .await
        .with_context(|| "Failed to bind listener to socket")?;
    log::info!("Listening (TCP) at {socket:?}", socket = args.socket);

    // Axum server
    let app = Router::new()
        .route("/ws", get(ws_handler))
        .layer(tower_http::cors::CorsLayer::permissive())
        .with_state(app_state.clone());

    let ws_addr = format!("0.0.0.0:{}", args.ws_port);
    let ws_listener = tokio::net::TcpListener::bind(&ws_addr).await?;
    log::info!("Listening (WS) at {ws_addr}");

    tokio::spawn(async move {
        if let Err(e) = axum::serve(ws_listener, app).await {
            log::error!("Axum server error: {e}");
        }
    });

    tokio::select! {
        // TCP Connections loop
        _ = async {
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
                        let stream: ClientStream = Box::pin(read.map(|msg| msg.map_err(|e| anyhow::anyhow!(e))));

                        tokio::spawn(handle_handshake(stream, sink, app_state.clone()));
                    }
                }
            }
        } => {},

        // Shutdown signal
        _ = shutdown_rx => {
            log::trace!("Shutdown singnal received");
        }
    }

    Ok(())
}
