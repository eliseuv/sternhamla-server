export type Player = 'Player1' | 'Player2';

export type HexIdx = [number, number];

export type MovementIndices = [HexIdx, HexIdx]; // [start, end]

export type Scores = [number, number];

export type GameResult =
    | {
        type: 'finished';
        winner: Player;
        total_turns: number;
        scores: Scores;
    }
    | {
        type: 'max_turns';
        total_turns: number;
        scores: Scores;
    };

export type RemoteOutMessage =
    | {
        type: 'welcome';
        session_id: string;
        player: Player;
    }
    | {
        type: 'reject';
        reason: string;
    }
    | {
        type: 'assign';
        player: Player;
    }
    | {
        type: 'disconnect';
    }
    | {
        type: 'turn';
        movements: MovementIndices[];
    }
    | {
        type: 'movement';
        player: Player;
        movement: MovementIndices;
        scores: Scores;
    }
    | {
        type: 'game_finished';
        result: GameResult;
    };

export type RemoteInMessage =
    | {
        type: 'hello';
    }
    | {
        type: 'reconnect';
        session_id: string;
    }
    | {
        type: 'choice';
        movement_index: number;
    };
