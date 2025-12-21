# Sternhalma Protocol Specification

This document specifies the communication protocol used between the Sternhalma Server and its clients.

## Overview

The protocol is message-based and uses **CBOR** (Concise Binary Object Representation) for serialization. All messages are serialized into CBOR before transmission.

### Transports

The server supports two transport methods, which differ in how messages are framed:

1. **Raw TCP** (Port 8080 default):
    * **Framing**: Messages are length-delimited.
    * **Header**: 4-byte big-endian integer specifying the length of the CBOR payload.
    * **Payload**: The CBOR-encoded message.

2. **WebSocket** (Port 8081 default, `/ws` endpoint):
    * **Framing**: Uses WebSocket Binary Frames.
    * **Payload**: Each WebSocket frame contains exactly one complete CBOR-encoded message.
    * **No additional length prefix** is used inside the frame.

## Message Flow

### Handshake

1. **Client Connects** (TCP or WebSocket).
2. **Client Sends**: `Hello` (for new session) or `Reconnect` (for existing session).
3. **Server Responds**:
    * `Welcome`: Connection accepted, session ID assigned.
    * `Reject`: Connection refused (e.g., server full, invalid session).
4. If accepted, Client is now ready to play. Note that the Client ALWAYS sees itself as "Player1".

### Game Loop

1. Server sends `Turn` to the active player with valid moves.
2. Active Client sends `Choice` with the selected move index.
3. Server broadcasts `Movement` to all clients to update board state.
4. Repeat until game end.
5. Server broadcasts `GameFinished`.

## Data Types

### Basic Types

* **UUID**: String (Canonical 8-4-4-4-12 format)
* **Player**: String enumeration: `"player1"`, `"player2"`
* **HexIdx**: Array of 2 integers `[q, r]` representing axial coordinates.
* **MovementIndices**: Array of 2 `HexIdx` `[start, end]`.
* **Scores**: Array of 2 integers `[score_p1, score_p2]`.

### GameResult

Object indicating the game outcome.

* **Finished**: `{ "type": "finished", "winner": Player, "total_turns": int, "scores": Scores }`
* **Max Turns**: `{ "type": "max_turns", "total_turns": int, "scores": Scores }`

## Client to Server Messages (`RemoteInMessage`)

These messages are sent from the Client to the Server.

### Hello

Request a new game session.

```json
{ "type": "hello" }
```

### Reconnect

Request to resume an existing session.

```json
{
  "type": "reconnect",
  "session_id": "UUID-STRING"
}
```

### Choice

Player selects a move from the available options provided in the last `Turn` message.

```json
{
  "type": "choice",
  "movement_index": 0  // unsigned integer
}
```

## Server to Client Messages (`RemoteOutMessage`)

These messages are sent from the Server to the Client.

### Welcome

Session established successfully.

```json
{
  "type": "welcome",
  "session_id": "UUID-STRING"
}
```

### Reject

Connection rejected.

```json
{
  "type": "reject",
  "reason": "Server full"
}
```

### Disconnection

Server is closing the connection.

```json
{ "type": "disconnect" }
```

### Turn

It is this client's turn to move. Contains all valid moves.

```json
{
  "type": "turn",
  "movements": [
    [[0, -4], [1, -5]], // Move 0: [start_q, start_r] -> [end_q, end_r]
    [[0, -4], [-1, -3]] // Move 1
    // ...
  ]
}
```

### Movement

A move has been made (by any player). Update the board.

```json
{
  "type": "movement",
  "player": "player1",
  "movement": [[0, -4], [1, -5]],
  "scores": [10, 5]
}
```

### GameFinished

The game has ended.

```json
{
  "type": "game_finished",
  "result": {
    "type": "finished",
    "winner": "player1",
    "total_turns": 42,
    "scores": [15, 10]
  }
}
```
