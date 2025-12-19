use crate::sternhalma::board::{movement::MovementIndices, player::Player};
use crate::sternhalma::{GameResult, Scores};
use anyhow::{Context, Result};
use bytes::{Bytes, BytesMut};
use serde::{Deserialize, Serialize};
use tokio_util::codec::{Decoder, Encoder, LengthDelimitedCodec};
use uuid::Uuid;

/// Maximum length of a remote message in bytes
pub const REMOTE_MESSAGE_LENGTH: usize = 4 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum RemoteOutMessage {
    /// Welcome message with session ID
    Welcome { session_id: Uuid, player: Player },
    /// Reconnect reject
    Reject { reason: String },
    /// Inform remote client about their assigned player
    Assign { player: Player },
    /// Disconnection signal
    Disconnect,
    /// Inform remote client that it is their turn
    Turn {
        /// List of available movements
        /// Each movement is represented by a pair of indices
        movements: Vec<MovementIndices>,
    },
    /// Inform remote client about a player's movement
    Movement {
        player: Player,
        movement: MovementIndices,
        scores: Scores,
    },
    /// Inform remote client that the game has finished with a result
    GameFinished { result: GameResult },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum RemoteInMessage {
    /// Hello - Request new session
    Hello,
    /// Reconnect - Request resume session
    Reconnect { session_id: Uuid },
    /// Movement made by player (index based)
    Choice { movement_index: usize },
}

impl RemoteInMessage {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        ciborium::from_reader(bytes).with_context(|| "Failed to deserialize remote message")
    }
}

// Codecs

#[derive(Debug)]
pub struct ServerCodec {
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
        let bytes = match self.delegate.decode(src)? {
            Some(bytes) => bytes,
            None => return Ok(None),
        };

        ciborium::from_reader(std::io::Cursor::new(bytes))
            .map(Some)
            .context("Failed to deserialize remote message")
    }
}

impl Encoder<RemoteOutMessage> for ServerCodec {
    type Error = anyhow::Error;

    fn encode(&mut self, item: RemoteOutMessage, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let mut buf = Vec::new();
        ciborium::into_writer(&item, &mut buf).context("Failed to serialize remote message")?;
        self.delegate
            .encode(Bytes::from(buf), dst)
            .context("Failed to frame message")
    }
}

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
