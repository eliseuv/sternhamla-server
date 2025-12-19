import { useState } from 'react';

interface WelcomeScreenProps {
    onJoin: (url: string) => void;
}

export function WelcomeScreen({ onJoin }: WelcomeScreenProps) {
    const [url, setUrl] = useState('ws://127.0.0.1:8081/ws');

    const handleSubmit = (e: React.FormEvent) => {
        e.preventDefault();
        if (url.trim()) {
            onJoin(url);
        }
    };

    return (
        <div className="card">
            <h1>Sternhalma</h1>
            <p>Enter the game server URL to connect.</p>
            <form onSubmit={handleSubmit} style={{ display: 'flex', flexDirection: 'column', gap: '1rem', alignItems: 'center' }}>
                <input
                    type="text"
                    value={url}
                    onChange={(e) => setUrl(e.target.value)}
                    placeholder="ws://localhost:8081/ws"
                    style={{
                        padding: '10px',
                        fontSize: '1rem',
                        width: '300px',
                        borderRadius: '4px',
                        border: '1px solid #ccc'
                    }}
                />
                <button type="submit" style={{ padding: '10px 20px', fontSize: '1rem', cursor: 'pointer' }}>
                    Join Game
                </button>
            </form>
        </div>
    );
}
