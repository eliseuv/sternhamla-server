use assert_matches::assert_matches;
use common::TestServer;
use sternhalma_server::protocol::{RemoteInMessage, RemoteOutMessage};

mod common;

#[test]
fn test_successful_connection_and_handshake() {
    let server = TestServer::new().expect("Failed to start server");

    let mut client = server.client().expect("Failed to connect client");

    // Send Hello
    client
        .send(RemoteInMessage::Hello)
        .expect("Failed to send Hello");

    // Expect Welcome
    let msg = client.recv().expect("Failed to receive response");
    assert_matches!(
        msg,
        RemoteOutMessage::Welcome {
            session_id: _,
            player: _
        }
    );
}

#[test]
fn test_multiple_players_connection() {
    let server = TestServer::new().expect("Failed to start server");

    // Player 1
    let mut client1 = server.client().expect("Failed to connect client 1");
    client1
        .send(RemoteInMessage::Hello)
        .expect("Failed to send Hello 1");
    let msg1 = client1.recv().expect("Failed to receive response 1");
    assert_matches!(msg1, RemoteOutMessage::Welcome { .. });

    // Player 2
    let mut client2 = server.client().expect("Failed to connect client 2");
    client2
        .send(RemoteInMessage::Hello)
        .expect("Failed to send Hello 2");
    let msg2 = client2.recv().expect("Failed to receive response 2");
    assert_matches!(msg2, RemoteOutMessage::Welcome { .. });
}

#[test]
fn test_reject_excess_players() {
    let server = TestServer::new().expect("Failed to start server");

    // Player 1
    let mut client1 = server.client().expect("Failed to connect client 1");
    client1.send(RemoteInMessage::Hello).unwrap();
    client1.recv().unwrap();

    // Player 2
    let mut client2 = server.client().expect("Failed to connect client 2");
    client2.send(RemoteInMessage::Hello).unwrap();
    client2.recv().unwrap();

    // Player 3 (Should be rejected)
    let mut client3 = server.client().expect("Failed to connect client 3");
    client3.send(RemoteInMessage::Hello).unwrap();
    let msg3 = client3.recv().expect("Failed to receive response 3");

    // Debug: print the message received
    println!("Player 3 received: {:?}", msg3);

    // Expect Reject
    assert_matches!(msg3, RemoteOutMessage::Reject { .. });
}
