import { useEffect, useState } from 'react';
import { AppRegistry } from 'react-native';
import { ScClient } from '@sc/data';
import { App } from '../App';
import { LoopbackTransport, type LoopbackTarget, parseTarget } from './transport';

/** Адрес ядра: сначала из хэша (`#p=PORT;t=TOKEN`, так его передаёт Servo-шелл),
 *  иначе dev-fallback — `/rpc-endpoint.json`, куда sc-rpc пишет `{port,token}`. */
function useCoreTarget(): LoopbackTarget | null | undefined {
  const [target, setTarget] = useState<LoopbackTarget | null | undefined>(() =>
    parseTarget(window.location.hash),
  );
  useEffect(() => {
    if (target) return;
    let alive = true;
    fetch('/rpc-endpoint.json')
      .then((r) => (r.ok ? r.json() : null))
      .then((j: { port?: number; token?: string } | null) => {
        if (!alive) return;
        setTarget(j?.port && j?.token ? { port: j.port, token: j.token } : null);
      })
      .catch(() => alive && setTarget(null));
    return () => {
      alive = false;
    };
  }, [target]);
  return target;
}

function Root() {
  const target = useCoreTarget();
  if (target === undefined) return null;
  if (!target) {
    return (
      <div style={{ color: 'rgba(255,255,255,0.7)', fontFamily: 'monospace', padding: 32 }}>
        Нет адреса ядра. Запусти sc-rpc (пишет /rpc-endpoint.json) или открой /#p=&lt;port&gt;;t=&lt;token&gt;.
      </div>
    );
  }
  const client = new ScClient(new LoopbackTransport(target));
  return <App client={client} />;
}

AppRegistry.registerComponent('SoundCloud', () => Root);
AppRegistry.runApplication('SoundCloud', {
  rootTag: document.getElementById('root'),
});
