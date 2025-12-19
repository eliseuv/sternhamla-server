use anyhow::{Context, Result, anyhow};
use std::process::{Child, Command, Stdio};
use std::sync::Once;
use std::thread;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use sternhalma_server::server::protocol::{ClientCodec, RemoteInMessage, RemoteOutMessage};
use tokio::net::TcpStream;
use tokio_util::codec::Framed;

static BUILD_SERVER: Once = Once::new();

pub struct TestServer {
    process: Child,
    pub address: String,
}

impl TestServer {
    pub fn new() -> Result<Self> {
        // Build the server binary once ensuring it's up to date
        BUILD_SERVER.call_once(|| {
            let status = Command::new("cargo")
                .args(["build", "--bin", "sternhalma-server"])
                .status()
                .expect("Failed to build server");
            assert!(status.success(), "Server build failed");
        });

        // Use a random port by letting the OS verify availability, or just use port 0 logic if supported by server?
        // Server takes "IP:PORT" or socket path.
        // We can pick a random port. To check availability, we can bind to 0 and get the port, then close.
        let port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
            listener.local_addr()?.port()
        };
        let address = format!("127.0.0.1:{}", port);

        // Spawn server
        let path = env!("CARGO_BIN_EXE_sternhalma-server");
        let mut process = Command::new(path)
            .arg("--socket")
            .arg(&address)
            .arg("--max-turns")
            .arg("100")
            .env("RUST_LOG", "debug")
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .context("Failed to spawn server")?;

        // Wait for "Listening" message on stdout or stderr (env_logger usually prints to stderr)
        // Sternhalma server header logs might be on stderr?
        // src/bin/sternhalma-server.rs: env_logger::init(); logic.
        // logs are typically stderr.
        // The server prints "Listening at ..." using log::info!.
        // Default env_logger writes to stderr.

        // Wait for server to be ready by polling the connection
        let mut attempts = 0;
        let max_attempts = 50;
        let mut started = false;

        while attempts < max_attempts {
            if std::net::TcpStream::connect(&address).is_ok() {
                started = true;
                break;
            }

            // Check if process is still running
            match process.try_wait() {
                Ok(Some(status)) => {
                    return Err(anyhow!(
                        "Server process exited early with status: {}",
                        status
                    ));
                }
                Ok(None) => {} // Still running
                Err(e) => return Err(anyhow!("Error checking server process status: {}", e)),
            }

            thread::sleep(Duration::from_millis(100));
            attempts += 1;
        }

        if !started {
            let _ = process.kill();
            return Err(anyhow!(
                "Server failed to accept connections within timeout"
            ));
        }

        // Give it a tiny bit more time to be fully ready accepting connections
        thread::sleep(Duration::from_millis(100));

        Ok(Self { process, address })
    }

    pub async fn client(&self) -> Result<TestClient> {
        let stream = TcpStream::connect(&self.address)
            .await
            .context("Failed to connect to server")?;
        Ok(TestClient {
            framed: Framed::new(stream, ClientCodec::new()),
        })
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

pub struct TestClient {
    framed: Framed<TcpStream, ClientCodec>,
}

impl TestClient {
    pub async fn send(&mut self, msg: RemoteInMessage) -> Result<()> {
        self.framed
            .send(msg)
            .await
            .context("Failed to send message")
    }

    pub async fn recv(&mut self) -> Result<RemoteOutMessage> {
        match self.framed.next().await {
            Some(Ok(msg)) => Ok(msg),
            Some(Err(e)) => Err(e),
            None => Err(anyhow!("Connection closed")),
        }
    }
}
