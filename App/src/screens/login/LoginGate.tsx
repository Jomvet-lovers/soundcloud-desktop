import { useCallback, useEffect, useRef, useState } from 'react';
import { Pressable, View } from 'react-native';
import { useSc } from '@sc/data';
import { Atmosphere, GlassSurface, heroGlass, Rise, ScText, useScTheme } from '@sc/ui';

type Stage = 'idle' | 'waiting' | 'error';

/** OAuth-вход: системный браузер + поллинг (порт флоу легаси Login). */
export function LoginGate({ onAuthed }: { onAuthed: () => void }) {
  const sc = useSc();
  const { accent } = useScTheme();
  const [stage, setStage] = useState<Stage>('idle');
  const [message, setMessage] = useState<string | null>(null);
  const polling = useRef(false);

  const start = useCallback(async () => {
    // окно открываем синхронно в жесте клика: window.open после await режет поп-ап-блокер
    const win = typeof window !== 'undefined' ? window.open('about:blank', '_blank') : null;
    setStage('waiting');
    setMessage(null);
    try {
      const { url, login_request_id } = await sc.auth.startLogin();
      if (win && !win.closed) win.location.href = url;
      else window.open(url, '_blank');
      polling.current = true;
      while (polling.current) {
        await new Promise((r) => setTimeout(r, 2000));
        const st = await sc.auth.pollLogin(login_request_id);
        if (st.session_id) {
          polling.current = false;
          await sc.auth.setSession(st.session_id);
          onAuthed();
          return;
        }
        if (st.error) {
          polling.current = false;
          setStage('error');
          setMessage(st.error);
          return;
        }
      }
    } catch (e) {
      if (win && !win.closed) win.close();
      setStage('error');
      setMessage(e instanceof Error ? e.message : String(e));
    }
  }, [sc, onAuthed]);

  useEffect(() => {
    return () => {
      polling.current = false;
    };
  }, []);

  return (
    <View style={{ flex: 1, height: '100%' as never, alignItems: 'center', justifyContent: 'center' }}>
      <Atmosphere />
      <Rise>
        <GlassSurface recipe={heroGlass} style={{ padding: 48, alignItems: 'center', gap: 20, minWidth: 380 }}>
          <ScText token="heroTitle" style={{ fontSize: 34, lineHeight: 38 }}>
            SoundCloud
          </ScText>
          <ScText level="secondary">Войди, чтобы течение началось</ScText>
          <Pressable
            onPress={start}
            disabled={stage === 'waiting'}
            style={{
              backgroundColor: accent.base,
              borderRadius: 9999,
              paddingHorizontal: 28,
              paddingVertical: 12,
              opacity: stage === 'waiting' ? 0.6 : 1,
            }}
          >
            <ScText style={{ color: accent.contrast, fontWeight: '600' }}>
              {stage === 'waiting' ? 'Жду подтверждения…' : 'Войти через SoundCloud'}
            </ScText>
          </Pressable>
          {message && (
            <ScText level="tertiary" style={{ maxWidth: 320, textAlign: 'center' }}>
              {message}
            </ScText>
          )}
        </GlassSurface>
      </Rise>
    </View>
  );
}
