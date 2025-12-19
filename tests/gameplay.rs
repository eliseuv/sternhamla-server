use common::TestServer;
use sternhalma_server::server::protocol::{RemoteInMessage, RemoteOutMessage};
use sternhalma_server::sternhalma::board::player::Player;

mod common;

#[tokio::test]
async fn test_gameplay_turn_and_move() {
    let server = TestServer::new().expect("Failed to start server");

    // Connect Player 1
    let mut client1 = server.client().await.expect("Failed to connect client 1");
    client1
        .send(RemoteInMessage::Hello)
        .await
        .expect("Failed to send Hello 1");
    let _welcome1 = client1.recv().await.expect("Failed to receive Welcome 1");
    // Consume Assign message
    let _assign1 = client1.recv().await.expect("Failed to receive Assign 1");

    // Connect Player 2
    let mut client2 = server.client().await.expect("Failed to connect client 2");
    client2
        .send(RemoteInMessage::Hello)
        .await
        .expect("Failed to send Hello 2");
    let _welcome2 = client2.recv().await.expect("Failed to receive Welcome 2");
    // Consume Assign message
    let _assign2 = client2.recv().await.expect("Failed to receive Assign 2");

    // Player 1 should receive Turn
    // Note: It might take a moment for game to start after players connect
    let msg_turn = client1
        .recv()
        .await
        .expect("Player 1 failed to receive Turn");

    // Extract available moves
    let movements = match msg_turn {
        RemoteOutMessage::Turn { movements } => movements,
        other => panic!("Expected Turn message for Player 1, got {:?}", other),
    };

    assert!(!movements.is_empty(), "Player 1 should have valid moves");

    // Player 1 makes a move (pick index 0)
    let move_index = 0;
    let expected_move = movements[0];

    client1
        .send(RemoteInMessage::Choice {
            movement_index: move_index,
        })
        .await
        .expect("Failed to send Choice");

    // Both players should receive Movement
    let msg_move1 = client1
        .recv()
        .await
        .expect("Player 1 failed to receive Movement broadcast");
    let msg_move2 = client2
        .recv()
        .await
        .expect("Player 2 failed to receive Movement broadcast");

    // Verify Movement message content
    // Verify Movement message content
    let _scores = match (msg_move1, msg_move2) {
        (
            RemoteOutMessage::Movement {
                player: p1,
                movement: m1,
                scores,
            },
            RemoteOutMessage::Movement {
                player: p2,
                movement: m2,
                scores: _,
            },
        ) => {
            assert_eq!(p1, Player::Player1);
            assert_eq!(p2, Player::Player1);
            assert_eq!(m1, expected_move);
            assert_eq!(m2, expected_move);
            scores
        }
        (m1, m2) => panic!("Expected Movement messages, got: {:?} and {:?}", m1, m2),
    };

    // Player 2 should receive Turn next
    let msg_turn2 = client2
        .recv()
        .await
        .expect("Player 2 failed to receive Turn");
    match msg_turn2 {
        RemoteOutMessage::Turn { movements } => {
            assert!(!movements.is_empty(), "Player 2 should have valid moves");
        }
        other => panic!("Player 2 expected Turn message, got {:?}", other),
    };
}
