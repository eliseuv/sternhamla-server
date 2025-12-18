use common::TestServer;
use std::mem::drop;
use sternhalma_server::protocol::{RemoteInMessage, RemoteOutMessage};

mod common;

#[test]
fn test_reconnection() {
    let server = TestServer::new().expect("Failed to start server");

    // Connect Player 1
    let mut client1 = server.client().expect("Failed to connect client 1");
    client1
        .send(RemoteInMessage::Hello)
        .expect("Failed to send Hello 1");
    let welcome1 = client1.recv().expect("Failed to receive Welcome 1");

    // Extract Session ID
    let session_id = match welcome1 {
        RemoteOutMessage::Welcome { session_id, .. } => session_id,
        _ => panic!("Expected Welcome message"),
    };

    // Consume Assign
    let _ = client1.recv().expect("Failed to receive Assign 1");

    // Connect Player 2 to start game logic (so we are in Playing state)
    let mut client2 = server.client().expect("Failed to connect client 2");
    client2
        .send(RemoteInMessage::Hello)
        .expect("Failed to send Hello 2");
    let _ = client2.recv().expect("Failed to receive Welcome 2");
    let _ = client2.recv().expect("Failed to receive Assign 2");

    // Player 1 receives Turn
    let _turn = client1.recv().expect("Player 1 failed to receive Turn");

    // Disconnect Player 1
    drop(client1);

    // Wait a bit to ensure server notices (or doesn't crash)
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Reconnect Player 1
    let mut client1_new = server.client().expect("Failed to connect client 1 again");
    client1_new
        .send(RemoteInMessage::Reconnect { session_id })
        .expect("Failed to send Reconnect");

    // Should fail if Server crashed or rejects.
    // Expect Welcome
    let msg = client1_new
        .recv()
        .expect("Failed to receive response after reconnect");
    match msg {
        RemoteOutMessage::Welcome {
            session_id: new_sid,
            player: _,
        } => {
            assert_eq!(session_id, new_sid, "Session ID should match");
        }
        RemoteOutMessage::Reject { reason } => panic!("Reconnection rejected: {}", reason),
        other => panic!("Unexpected message after reconnect: {:?}", other),
    }

    // Should receive Assign again
    let _assign = client1_new
        .recv()
        .expect("Failed to receive Assign after reconnect");

    // Should receive Turn again?
    // According to server logic, if it was My Turn, and I reconnect, server resends Turn.
    let turn_again = client1_new
        .recv()
        .expect("Failed to receive Turn after reconnect");
    match turn_again {
        RemoteOutMessage::Turn { .. } => {}
        other => panic!("Expected Turn after reconnect, got {:?}", other),
    }
}
