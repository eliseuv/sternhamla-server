//! # Protocol Module
//!
//! This module defines the external communication protocol used over the network.
//! It specifies the messages exchanged with remote clients (via TCP or WebSocket).
//!
//! ## Messages
//! - [`RemoteOutMessage`]: Messages sent from Server to Remote Client.
//! - [`RemoteInMessage`]: Messages sent from Remote Client to Server.
//!
//! ## Codecs
//! It also includes `tokio_util` codecs ([`ServerCodec`], [`ClientCodec`]) for framing and serialization (CBOR).

use crate::sternhalma::board::{movement::MovementIndices, player::Player};
use crate::sternhalma::{GameResult, Scores};
use anyhow::{Context, Result};
use bytes::{Bytes, BytesMut};
use serde::{Deserialize, Serialize};
use tokio_util::codec::{Decoder, Encoder, LengthDelimitedCodec};
use uuid::Uuid;

/// Maximum length of a remote message in bytes
///
/// This limits the size of individual messages to prevent DoS attacks.
pub const REMOTE_MESSAGE_LENGTH: usize = 4 * 1024;

/// Messages sent from the Server to a Remote Client
///
/// This enum defines all possible messages that the server can send to a connected client
/// over the network. It uses `serde` for serialization to JSON (or CBOR/etc depending on use).
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum RemoteOutMessage {
    /// Welcome message with session ID
    ///
    /// Sent immediately after connection to assign a session ID.
    ///
    /// # Design Decision
    /// The player identity is NOT sent here because the protocol ensures every client
    /// sees themselves as `Player1`. The server handles the mapping to the actual
    /// internal player identity.
    Welcome { session_id: Uuid },
    /// Reconnect reject
    ///
    /// Sent if a reconnection attempt fails (e.g., invalid session ID).
    Reject { reason: String },
    /// Disconnection signal
    ///
    /// Sent to serve as a polite "goodbye" before closing the connection.
    Disconnect,
    /// Inform remote client that it is their turn
    Turn {
        /// List of available movements
        /// Each movement is represented by a pair of indices
        movements: Vec<MovementIndices>,
    },
    /// Inform remote client about a player's movement
    ///
    /// Sent to update the client's view of the game board.
    Movement {
        player: Player,
        movement: MovementIndices,
        scores: Scores,
    },
    /// Inform remote client that the game has finished with a result
    GameFinished { result: GameResult },
}

/// Messages sent from a Remote Client to the Server
///
/// This enum defines all valid messages a client can send to the server.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum RemoteInMessage {
    /// Hello - Request new session
    ///
    /// Sent by a new client to initiate a connection.
    Hello,
    /// Reconnect - Request resume session
    ///
    /// Sent by a client trying to resume a previous session.
    Reconnect { session_id: Uuid },
    /// Movement made by player (index based)
    ///
    /// Sent when the user selects a move. The index corresponds to the list
    /// of valid moves sent in the `Turn` message.
    Choice { movement_index: usize },
}

impl RemoteInMessage {
    /// deserializes a `RemoteInMessage` from a byte slice using `ciborium` (CBOR).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        ciborium::from_reader(bytes).with_context(|| "Failed to deserialize remote message")
    }
}

// Codecs

/// Server-side Codec
///
/// Handles framing (length-delimited) and serialization/deserialization for the server.
/// Decodes `RemoteInMessage` and Encodes `RemoteOutMessage`.
#[derive(Debug)]
pub struct ServerCodec {
    /// Underlying framing codec
    delegate: LengthDelimitedCodec,
}

impl ServerCodec {
    pub fn new() -> Self {
        Self {
            delegate: LengthDelimitedCodec::new(),
        }
    }
}

impl Default for ServerCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder for ServerCodec {
    type Item = RemoteInMessage;
    type Error = anyhow::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // Decode the frame first
        let bytes = match self.delegate.decode(src)? {
            Some(bytes) => bytes,
            None => return Ok(None),
        };

        // Deserialize the payload
        ciborium::from_reader(std::io::Cursor::new(bytes))
            .map(Some)
            .context("Failed to deserialize remote message")
    }
}

impl Encoder<RemoteOutMessage> for ServerCodec {
    type Error = anyhow::Error;

    fn encode(&mut self, item: RemoteOutMessage, dst: &mut BytesMut) -> Result<(), Self::Error> {
        // Serialize the payload
        let mut buf = Vec::new();
        ciborium::into_writer(&item, &mut buf).context("Failed to serialize remote message")?;

        // Frame the payload
        self.delegate
            .encode(Bytes::from(buf), dst)
            .context("Failed to frame message")
    }
}

/// Client-side Codec
///
/// Handles framing (length-delimited) and serialization/deserialization for the client (used in tests/bots).
/// Decodes `RemoteOutMessage` and Encodes `RemoteInMessage`.
#[derive(Debug)]
pub struct ClientCodec {
    delegate: LengthDelimitedCodec,
}

impl ClientCodec {
    pub fn new() -> Self {
        Self {
            delegate: LengthDelimitedCodec::new(),
        }
    }
}

impl Default for ClientCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder for ClientCodec {
    type Item = RemoteOutMessage;
    type Error = anyhow::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let bytes = match self.delegate.decode(src)? {
            Some(bytes) => bytes,
            None => return Ok(None),
        };

        ciborium::from_reader(std::io::Cursor::new(bytes))
            .map(Some)
            .context("Failed to deserialize remote message")
    }
}

impl Encoder<RemoteInMessage> for ClientCodec {
    type Error = anyhow::Error;

    fn encode(&mut self, item: RemoteInMessage, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let mut buf = Vec::new();
        ciborium::into_writer(&item, &mut buf).context("Failed to serialize remote message")?;
        self.delegate
            .encode(Bytes::from(buf), dst)
            .context("Failed to frame message")
    }
}
