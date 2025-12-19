import { useEffect, useState } from 'react';
import useWebSocket, { ReadyState } from 'react-use-websocket';
import { Board } from './Board';
import { PLAYER1_STARTING_POSITIONS, PLAYER2_STARTING_POSITIONS } from './constants';
import type { RemoteInMessage, RemoteOutMessage, Player, MovementIndices, Scores } from './protocol';
import { decode, encode } from 'cbor-x';

interface GameProps {
    url: string;
    onLeave?: () => void;
}

export function Game({ url, onLeave }: GameProps) {
    console.log("Game rendering", url);
    const [boardState, setBoardState] = useState<Map<string, Player>>(new Map());
    const [myPlayer, setMyPlayer] = useState<Player | null>(null);
    const [sessionId, setSessionId] = useState<string | null>(null);
    const [turn, setTurn] = useState<MovementIndices[] | null>(null);
    const [lastMove, setLastMove] = useState<MovementIndices | undefined>(undefined);
    const [scores, setScores] = useState<Scores>([0, 0]);
    const [gameResult, setGameResult] = useState<any>(null);
    const [connectionError, setConnectionError] = useState<string | null>(null);

    const { sendMessage, lastMessage, readyState } = useWebSocket(url, {
        shouldReconnect: () => true,
        reconnectAttempts: 10,
        reconnectInterval: 3000,
        onOpen: () => {
            console.log('Connected');
            // Handshake
            if (sessionId) {
                send({ type: 'reconnect', session_id: sessionId });
            } else {
                send({ type: 'hello' });
            }
        },
        onError: () => setConnectionError('Connection Error'),
    });

    // Helper to send CBOR
    const send = (msg: RemoteInMessage) => {
        const data = encode(msg);
        sendMessage(data);
    };

    const handleMessage = (msg: RemoteOutMessage) => {
        switch (msg.type) {
            case 'welcome':
                setSessionId(msg.session_id);
                setMyPlayer(msg.player);
                setConnectionError(null);
                break;
            case 'reject':
                setConnectionError(`Connection Rejected: ${msg.reason}`);
                setSessionId(null);
                break;
            case 'assign':
                setMyPlayer(msg.player);
                break;
            case 'turn':
                setTurn(msg.movements);
                break;
            case 'movement':
                // Update board
                const { player, movement, scores } = msg;
                setScores(scores);
                setLastMove(movement);
                setTurn(null);

                setBoardState(prev => {
                    const next = new Map(prev);
                    const startKey = `${movement[0][0]},${movement[0][1]}`;
                    const endKey = `${movement[1][0]},${movement[1][1]}`;

                    // Verify coherence
                    if (next.get(startKey) !== player) {
                        console.warn("Movement received for piece not at expected location", startKey, next.get(startKey), player);
                    }
                    console.log(`Moving ${player} from ${startKey} to ${endKey}`);
                    next.delete(startKey);
                    next.set(endKey, player);
                    console.log("New board state size:", next.size);
                    return next;
                });
                break;
            case 'game_finished':
                setGameResult(msg.result);
                break;
            case 'disconnect':
                setConnectionError("Opponent Disconnected");
                break;
        }
    };

    // Handle incoming messages
    useEffect(() => {
        if (lastMessage) {
            if (lastMessage.data instanceof Blob) {
                lastMessage.data.arrayBuffer().then((buf: ArrayBuffer) => {
                    try {
                        const msg = decode(new Uint8Array(buf)) as RemoteOutMessage;
                        console.log('Received:', msg);
                        handleMessage(msg);
                    } catch (e) {
                        console.error("Failed to decode message", e);
                    }
                });
            } else {
                console.warn("Received non-blob message", lastMessage.data);
            }
        }
    }, [lastMessage]);

    // Initial board setup
    useEffect(() => {
        const initialMap = new Map<string, Player>();
        PLAYER1_STARTING_POSITIONS.forEach(([i, j]) => initialMap.set(`${i},${j}`, 'Player1'));
        PLAYER2_STARTING_POSITIONS.forEach(([i, j]) => initialMap.set(`${i},${j}`, 'Player2'));
        setBoardState(initialMap);
    }, []);

    useEffect(() => {
        (window as any).getMoves = () => turn;
        (window as any).makeMove = (index: number) => {
            console.log("Auto-moving index", index);
            send({ type: 'choice', movement_index: index });
            setTurn(null);
        };
    }, [turn]);

    const handleMoveSelect = (index: number) => {
        send({ type: 'choice', movement_index: index });
        setTurn(null);
    };

    const connectionStatus = {
        [ReadyState.CONNECTING]: 'Connecting',
        [ReadyState.OPEN]: 'Open',
        [ReadyState.CLOSING]: 'Closing',
        [ReadyState.CLOSED]: 'Closed',
        [ReadyState.UNINSTANTIATED]: 'Uninstantiated',
    }[readyState];

    return (
        <div className="card">
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                <h1>Sternhalma</h1>
                {onLeave && <button onClick={onLeave} style={{ padding: '5px 10px' }}>Disconnect</button>}
            </div>

            <div style={{ marginBottom: 10, width: '100%', display: 'flex', justifyContent: 'space-between', padding: '0 20px', boxSizing: 'border-box' }}>
                <span>Status: <strong style={{ color: connectionError ? '#ef4444' : '#10b981' }}>{connectionStatus}</strong> {connectionError && <span style={{ color: '#ef4444' }}>({connectionError})</span>}</span>
                {myPlayer && (
                    <span>You are: <strong style={{ color: myPlayer === 'Player1' ? '#8b5cf6' : '#f59e0b' }}>{myPlayer}</strong></span>
                )}
            </div>

            <div style={{ marginBottom: 10, width: '100%', display: 'flex', justifyContent: 'center', gap: '20px', fontSize: '1.2rem' }}>
                <span style={{ color: '#8b5cf6' }}>P1: {scores[0]}</span>
                <span style={{ color: '#555' }}>|</span>
                <span style={{ color: '#f59e0b' }}>P2: {scores[1]}</span>
            </div>

            {turn && <div style={{ color: '#10b981', fontWeight: 'bold', fontSize: '1.1rem' }}>Your Turn!</div>}
            {gameResult && (
                <div style={{ color: '#ef4444', fontWeight: 'bold', fontSize: '1.5rem' }}>
                    Game Over! {gameResult.type === 'finished' ? `Winner: ${gameResult.winner}` : 'Max Turns Reached (Draw)'}
                </div>
            )}

            <div style={{ display: 'flex', justifyContent: 'center', marginTop: 20 }}>
                <Board
                    boardState={boardState}
                    availableMoves={turn || undefined}
                    onMoveSelect={handleMoveSelect}
                    lastMove={lastMove}
                    myPlayer={myPlayer || undefined}
                />
            </div>
        </div>
    );
}
