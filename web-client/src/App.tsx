import { useState } from 'react';
import { Game } from './Game';
import { WelcomeScreen } from './WelcomeScreen';

function App() {
  const [activeUrl, setActiveUrl] = useState<string | null>(null);

  return (
    <div style={{ width: '100%', minHeight: '100vh', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
      {!activeUrl ? (
        <WelcomeScreen onJoin={setActiveUrl} />
      ) : (
        <Game url={activeUrl} onLeave={() => setActiveUrl(null)} />
      )}
    </div>
  );
}

export default App;

