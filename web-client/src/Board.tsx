import React from 'react';
import { VALID_POSITIONS } from './constants';
import type { HexIdx } from './constants';
import type { Player, MovementIndices } from './protocol';

interface BoardProps {
    boardState: Map<string, Player>;
    availableMoves?: MovementIndices[];
    onMoveSelect?: (index: number) => void;
    lastMove?: MovementIndices;
    myPlayer?: Player;
}

const HEX_SIZE = 24;

// Conversion to pixel
const hexToPixel = (i: number, j: number) => {
    // Standard pointy-topped math
    // x = size * sqrt(3) * (q + r/2)
    // y = size * 3/2 * r
    // Mapping: i -> r (row), j -> q (col)
    const x = HEX_SIZE * Math.sqrt(3) * (j + i / 2);
    const y = HEX_SIZE * 3 / 2 * i;
    return { x, y };
};

export const Board: React.FC<BoardProps> = ({ boardState, availableMoves, onMoveSelect, lastMove }) => {
    // 1. Calculate Bounds
    let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
    VALID_POSITIONS.forEach(([i, j]) => {
        const { x, y } = hexToPixel(i, j);
        minX = Math.min(minX, x);
        maxX = Math.max(maxX, x);
        minY = Math.min(minY, y);
        maxY = Math.max(maxY, y);
    });

    const padding = HEX_SIZE * 2;
    minX -= padding;
    maxX += padding;
    minY -= padding;
    maxY += padding;
    const width = maxX - minX;
    const height = maxY - minY;

    // State for local selection (Start Hex)
    const [selectedStart, setSelectedStart] = React.useState<HexIdx | null>(null);

    const handleHexClick = (i: number, j: number) => {
        if (!availableMoves) return;

        // Check if we clicked a target for the currently selected start
        if (selectedStart) {
            const moveIndex = availableMoves.findIndex(m =>
                m[0][0] === selectedStart[0] && m[0][1] === selectedStart[1] &&
                m[1][0] === i && m[1][1] === j
            );
            if (moveIndex !== -1) {
                onMoveSelect?.(moveIndex);
                setSelectedStart(null);
                return;
            }
        }

        // Check if we clicked a valid start point (a piece that has moves)
        const movesFromHere = availableMoves.some(m => m[0][0] === i && m[0][1] === j);
        if (movesFromHere) {
            // Toggle
            if (selectedStart && selectedStart[0] === i && selectedStart[1] === j) {
                setSelectedStart(null);
            } else {
                setSelectedStart([i, j]);
            }
        } else {
            setSelectedStart(null);
        }
    };

    const renderHex = (i: number, j: number) => {
        const { x, y } = hexToPixel(i, j);
        const key = `${i},${j}`;
        const player = boardState.get(key);

        const isStart = selectedStart && selectedStart[0] === i && selectedStart[1] === j;
        const isTarget = selectedStart && availableMoves?.some(m =>
            m[0][0] === selectedStart[0] && m[0][1] === selectedStart[1] &&
            m[1][0] === i && m[1][1] === j
        );
        const isStartCandidate = !selectedStart && availableMoves?.some(m => m[0][0] === i && m[0][1] === j);

        // -- Styles --

        // Base Hexagon (The "Hole")
        let hexFill = '#333';
        let hexStroke = '#444';
        let hexStrokeWidth = 1;

        if (isStart) {
            hexStroke = '#3b82f6'; // Selected Blue
            hexStrokeWidth = 2;
        } else if (isTarget) {
            hexStroke = '#10b981'; // Target Green
            hexFill = '#064e3b';   // Dark Green background
            hexStrokeWidth = 2;
        } else if (isStartCandidate) {
            hexStroke = '#6366f1'; // Candidate Indigo
            hexStrokeWidth = 1.5;
        }

        // Piece (The "Marble")
        const hasPiece = player === 'Player1' || player === 'Player2';
        let marbleFill = null;
        let marbleOpacity = 1;

        if (hasPiece) {
            marbleFill = player === 'Player1' ? '#8b5cf6' : '#f59e0b';
        } else if (isTarget) {
            marbleFill = '#10b981';
            marbleOpacity = 0.3;
        }


        // Last Move Highlight
        // If this hex was the FROM or TO of the last move
        let highlightCircle = null;
        if (lastMove) {
            const isFrom = lastMove[0][0] === i && lastMove[0][1] === j;
            const isTo = lastMove[1][0] === i && lastMove[1][1] === j;
            if (isFrom || isTo) {
                highlightCircle = <circle cx={x} cy={y} r={HEX_SIZE * 0.85} fill="none" stroke="#ef4444" strokeWidth="2" strokeDasharray="4,2" opacity="0.7" />;
            }
        }

        // Points for Hexagon
        const points = [];
        for (let k = 0; k < 6; k++) {
            const angle_deg = 60 * k - 30;
            const angle_rad = Math.PI / 180 * angle_deg;
            points.push(`${x + HEX_SIZE * Math.cos(angle_rad)},${y + HEX_SIZE * Math.sin(angle_rad)}`);
        }

        // Marble Radius
        const r = HEX_SIZE * 0.6;

        return (
            <g key={key} data-hex={`${i},${j}`} onClick={() => handleHexClick(i, j)} style={{ cursor: isStartCandidate || isTarget ? 'pointer' : 'default' }}>
                {/* Hexagon Background (Hole) */}
                <polygon points={points.join(' ')} fill={hexFill} stroke={hexStroke} strokeWidth={hexStrokeWidth} />

                {/* Marble */}
                {marbleFill && (
                    <>
                        {hasPiece && <circle cx={x} cy={y + 2} r={r} fill="rgba(0,0,0,0.5)" pointerEvents="none" />}
                        <circle cx={x} cy={y} r={r} fill={marbleFill} fillOpacity={marbleOpacity} stroke="none" pointerEvents="none" />
                        {/* Shine */}
                        {hasPiece && <circle cx={x - r * 0.3} cy={y - r * 0.3} r={r * 0.25} fill="rgba(255,255,255,0.2)" pointerEvents="none" />}
                    </>
                )}

                {highlightCircle}

                {highlightCircle}

                {/* Debug Text */}
                {/* <text x={x} y={y} textAnchor="middle" dy=".3em" fontSize="6" fill="#777">{i},{j}</text> */}
            </g>
        );
    };

    return (
        <svg width={width} height={height} viewBox={`${minX} ${minY} ${width} ${height}`} style={{ filter: 'drop-shadow(0 0 10px rgba(0,0,0,0.5))' }}>
            {VALID_POSITIONS.map(([i, j]) => renderHex(i, j))}
        </svg>
    );
};
