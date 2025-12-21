use common::TestServer;
use std::mem::drop;
use sternhalma_server::server::protocol::{RemoteInMessage, RemoteOutMessage};
use sternhalma_server::sternhalma::board::player::Player;

mod common;

#[tokio::test]
async fn test_reconnection() {
    let server = TestServer::new().expect("Failed to start server");

    // Connect Client 1
    let mut client1 = server.client().await.expect("Failed to connect client 1");
    client1
        .send(RemoteInMessage::Hello)
        .await
        .expect("Failed to send Hello 1");
    let msg_welcome = client1.recv().await.expect("Failed to receive Welcome 1");

    let session_id = match msg_welcome {
        RemoteOutMessage::Welcome { session_id } => session_id,
        other => panic!("Expected Welcome, got: {:?}", other),
    };

    // Connect Client 2 to start game
    let mut client2 = server.client().await.expect("Failed to connect client 2");
    client2
        .send(RemoteInMessage::Hello)
        .await
        .expect("Failed to send Hello 2");
    let _ = client2.recv().await.expect("Failed to receive Welcome 2");

    // Client 1 receives Turn (wait for it to be sure game started)
    let _turn = client1
        .recv()
        .await
        .expect("Client 1 failed to receive Turn");

    // Simulate Client 1 Disconnect (drop client)
    drop(client1);

    // Reconnect Client 1
    let mut client1_new = server
        .client()
        .await
        .expect("Failed to connect client 1 again");
    client1_new
        .send(RemoteInMessage::Reconnect { session_id })
        .await
        .expect("Failed to send Reconnect");

    // Expect Welcome
    let msg = client1_new
        .recv()
        .await
        .expect("Failed to receive response after reconnect");
    match msg {
        RemoteOutMessage::Welcome {
            session_id: new_sid,
        } => {
            assert_eq!(session_id, new_sid, "Session ID should match");
        }
        RemoteOutMessage::Reject { reason } => panic!("Reconnection rejected: {}", reason),
        other => panic!("Unexpected message after reconnect: {:?}", other),
    }

    // Should receive Turn again?
    // According to server logic, if it was My Turn, and I reconnect, server resends Turn.
    let turn_again = client1_new
        .recv()
        .await
        .expect("Failed to receive Turn after reconnect");
    match turn_again {
        RemoteOutMessage::Turn { .. } => {}
        other => panic!("Expected Turn after reconnect, got {:?}", other),
    }
}
