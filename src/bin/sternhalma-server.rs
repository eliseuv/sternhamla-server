use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use tokio::{
    net::TcpListener,
    sync::{broadcast, mpsc, oneshot},
};

use axum::{Router, routing::get};
use futures::{SinkExt, StreamExt};
use sternhalma_server::server::{
    MainThreadMessage, Server,
    client::{ClientSink, ClientStream},
    handshake::{AppState, handle_handshake},
    messages::{ClientMessage, ServerBroadcast},
    protocol::ServerCodec,
    ws::ws_handler,
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
