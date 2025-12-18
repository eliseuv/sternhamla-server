use common::TestServer;
use sternhalma_server::protocol::{RemoteInMessage, RemoteOutMessage};
use sternhalma_server::sternhalma::board::player::Player;

mod common;

#[test]
fn test_gameplay_turn_and_move() {
    let server = TestServer::new().expect("Failed to start server");

    // Connect Player 1
    let mut client1 = server.client().expect("Failed to connect client 1");
    client1
        .send(RemoteInMessage::Hello)
        .expect("Failed to send Hello 1");
    let _welcome1 = client1.recv().expect("Failed to receive Welcome 1");
    // Consume Assign message
    let _assign1 = client1.recv().expect("Failed to receive Assign 1");

    // Connect Player 2
    let mut client2 = server.client().expect("Failed to connect client 2");
    client2
        .send(RemoteInMessage::Hello)
        .expect("Failed to send Hello 2");
    let _welcome2 = client2.recv().expect("Failed to receive Welcome 2");
    // Consume Assign message
    let _assign2 = client2.recv().expect("Failed to receive Assign 2");

    // Player 1 should receive Turn
    // Note: It might take a moment for game to start after players connect
    let msg_turn = client1.recv().expect("Player 1 failed to receive Turn");

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
        .expect("Failed to send Choice");

    // Both players should receive Movement
    let msg_move1 = client1
        .recv()
        .expect("Player 1 failed to receive Movement broadcast");
    let msg_move2 = client2
        .recv()
        .expect("Player 2 failed to receive Movement broadcast");

    // Verify Movement message content
    match msg_move1 {
        RemoteOutMessage::Movement {
            player,
            movement,
            scores: _,
        } => {
            assert_eq!(player, Player::Player1);
            assert_eq!(movement, expected_move);
            // Scores might change if move is scoring, but initially likely 0?
            // Sternhalma goal is opposite side.
        }
        other => panic!("Player 1 expected Movement message, got {:?}", other),
    }

    match msg_move2 {
        RemoteOutMessage::Movement {
            player,
            movement,
            scores: _,
        } => {
            assert_eq!(player, Player::Player1);
            assert_eq!(movement, expected_move);
        }
        other => panic!("Player 2 expected Movement message, got {:?}", other),
    }

    // Player 2 should receive Turn next
    let msg_turn2 = client2.recv().expect("Player 2 failed to receive Turn");
    match msg_turn2 {
        RemoteOutMessage::Turn { movements } => {
            assert!(!movements.is_empty(), "Player 2 should have valid moves");
        }
        other => panic!("Player 2 expected Turn message, got {:?}", other),
    };
}
