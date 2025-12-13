# Sternhalma game server

## The Sternhalma game

The game is played on an hexagonal board in the shape of a six point star with
121 valid positions.
Each player has 15 pieces and start on the opposite sides of the board.
The pieces are represented by colored circles, one color for each player.

```text
   󠀠󠀠󠀠󠀠   󠀠󠀠󠀠󠀠   󠀠󠀠󠀠󠀠   󠀠󠀠󠀠󠀠     󠀠󠀠󠀠󠀠   󠀠󠀠󠀠󠀠   🔴
    󠀠󠀠󠀠󠀠   󠀠󠀠󠀠󠀠   󠀠󠀠󠀠󠀠   󠀠󠀠󠀠󠀠     󠀠󠀠󠀠󠀠   🔴 🔴
  󠀠󠀠󠀠󠀠   󠀠󠀠󠀠󠀠   󠀠󠀠󠀠󠀠   󠀠󠀠󠀠󠀠   󠀠󠀠󠀠󠀠     🔴 🔴 🔴
   󠀠󠀠󠀠󠀠   󠀠󠀠󠀠󠀠   󠀠󠀠󠀠󠀠   󠀠󠀠󠀠󠀠   󠀠󠀠󠀠󠀠  🔴 🔴 🔴 🔴
   ⚫ ⚫ ⚫ ⚫ 🔴 🔴 🔴 🔴 🔴 ⚫ ⚫ ⚫ ⚫
  󠀠󠀠󠀠󠀠   ⚫ ⚫ ⚫ ⚫ ⚫ ⚫ ⚫ ⚫ ⚫ ⚫ ⚫ ⚫
   󠀠󠀠󠀠󠀠   ⚫ ⚫ ⚫ ⚫ ⚫ ⚫ ⚫ ⚫ ⚫ ⚫ ⚫
       ⚫ ⚫ ⚫ ⚫ ⚫ ⚫ ⚫ ⚫ ⚫ ⚫
  󠀠󠀠󠀠󠀠   󠀠󠀠󠀠󠀠   ⚫ ⚫ ⚫ ⚫ ⚫ ⚫ ⚫ ⚫ ⚫
   󠀠󠀠󠀠󠀠   ⚫ ⚫ ⚫ ⚫ ⚫ ⚫ ⚫ ⚫ ⚫ ⚫
    ⚫ ⚫ ⚫ ⚫ ⚫ ⚫ ⚫ ⚫ ⚫ ⚫ ⚫
  ⚫ ⚫ ⚫ ⚫ ⚫ ⚫ ⚫ ⚫ ⚫ ⚫ ⚫ ⚫
⚫ ⚫ ⚫ ⚫ 🔵 🔵 🔵 🔵 🔵 ⚫ ⚫ ⚫ ⚫
 󠀠󠀠󠀠󠀠   󠀠󠀠󠀠󠀠   󠀠󠀠󠀠󠀠   󠀠󠀠󠀠󠀠   🔵 🔵 🔵 🔵
  󠀠󠀠󠀠󠀠   󠀠󠀠󠀠󠀠   󠀠󠀠󠀠󠀠   󠀠󠀠󠀠󠀠   🔵 🔵 🔵
   󠀠󠀠󠀠󠀠   󠀠󠀠󠀠󠀠   󠀠󠀠󠀠󠀠   󠀠󠀠󠀠󠀠   🔵 🔵
    󠀠󠀠󠀠󠀠   󠀠󠀠󠀠󠀠   󠀠󠀠󠀠󠀠   󠀠󠀠󠀠󠀠   🔵
```

The game is turn based and a player can move one piece per turn.
A piece can be moved to an adjacent empty position or jump over an arbitrary
number of single pieces from any player.

The goal of the game is to move all one's pieces to the opposite side of the board.

## Architecture

The server is built using **Rust** and **Tokio**, leveraging an asynchronous, actor-like architecture to ensure high performance and scalability. This design decouples connection handling/IO from the core game logic.

### Key Components

* **Main Task**: Responsible for initializing the server, binding the Unix Listener, and accepting incoming connections. For each connection, it spawns a dedicated Client Task.
* **Server Task**: The central authority of the game session. It maintains the `Game` state, validates moves, manages turn order, and broadcasts updates to all clients.
* **Client Task**: Acts as a bridge between the Server Task and the external Player (remote client). It handles serialization/deserialization of network messages and forwards requests/events between the socket and the internal channels.

### Scalability & Design Philosophy

The server is designed to be **client-agnostic**. It does not enforce any specific UI implementation; any client (CLI, TUI, GUI, AI agent) that implements the communication protocol can connect and play.

Due to its asynchronous nature, the server handles I/O efficiently, sleeping when idle. While currently configured for a single game session, the architecture is designed to support **multiple concurrent game sessions** in future iterations, where a central "Lobby" actor could spawn independent Server Tasks for each match.

```mermaid
graph TD
    subgraph "Sternhalma Server Process"
        Main[Main Listener]
        Server[Server Task (Game Logic)]
        
        subgraph "Client Tasks"
            C1[Client Task 1]
            C2[Client Task 2]
        end
    end

    subgraph "External World"
        P1[Player 1 Client]
        P2[Player 2 Client]
    end

    Main -->|Spawns| C1
    Main -->|Spawns| C2
    Main -->|Spawns| Server

    C1 <-->|Unix Domain Socket| P1
    C2 <-->|Unix Domain Socket| P2

    C1 <-->|MPSC / Broadcast| Server
    C2 <-->|MPSC / Broadcast| Server
```

## Communication Protocol

The server communicates via **Unix Domain Sockets** (files system based sockets), ensuring low-latency IPC for local clients.

* **Serialization**: Messages are serialized using **CBOR** (Concise Binary Object Representation).
* **Framing**: Each message is prefixed with a 4-byte big-endian integer indicating the payload length.

### Message Types

#### Server -> Client

* `assign`: Tells the client which Player ID they control.
* `turn`: Notifies the client it is their turn and provides a list of valid moves.
* `movement`: Broadcasts a move made by any player to update local state.
* `game_finished`: Declares the game over, with winner and stats.
* `disconnect`: Signals the session is ending.

#### Client -> Server

* `choice`: The client selects a move (responding to a `turn` message).
* `disconnect`: The client is leaving.

## Usage

The server executable is `sternhalma-server`.

```bash
sternhalma-server [OPTIONS] --socket <PATH>
```

### Arguments

* `-s, --socket <PATH>`: Filesystem path to bind the Unix Socket.
* `-n, --max-turns <N>`: (Optional) Limit the game to N turns.
* `-t, --timeout <SECONDS>`: (Optional) Connection timeout in seconds (default: 30).
