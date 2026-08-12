import { useEffect, useState } from 'react';
import { ActivityIndicator, Pressable, TextInput, View } from 'react-native';
import { type Comment, type Me, type Urn, useSc } from '@sc/data';
import { ClockIcon, formatDuration, ScText, SendIcon } from '@sc/ui';
import { getPositionSecs, useWholeSeconds } from '../../player/position';
import type { TrackAura } from './useTrackAura';

export function CommentComposer({ trackUrn, isCurrent, aura, onPosted }: {
  trackUrn: string;
  isCurrent: boolean;
  aura: TrackAura;
  onPosted: (comment: Comment) => void;
}) {
  const sc = useSc();
  const [me, setMe] = useState<Me | null>(null);
  const [body, setBody] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const secs = useWholeSeconds();

  useEffect(() => {
    let alive = true;
    void sc.me.profile().then((m) => { if (alive) setMe(m); }).catch(() => {});
    return () => { alive = false; };
  }, [sc]);

  const submit = async () => {
    const text = body.trim();
    if (!text || busy) return;
    setBusy(true);
    setError(null);
    const ts = isCurrent ? Math.floor(Math.max(0, getPositionSecs()) * 1000) : null;
    try {
      await sc.tracks.postComment(trackUrn as Urn, text, ts);
      onPosted({
        id: -Date.now(),
        body: text,
        timestamp_ms: ts,
        created_at: new Date().toISOString(),
        user: { id: me?.id ?? '', username: me?.username ?? 'Вы', avatar_url: me?.avatar_url ?? null, permalink_url: me?.permalink_url ?? null },
      });
      setBody('');
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <View style={{ borderRadius: 18, paddingHorizontal: 16, paddingVertical: 12, backgroundColor: 'rgba(255,255,255,0.045)', borderWidth: 1, borderColor: 'rgba(255,255,255,0.08)' }}>
      {isCurrent && (
        <View style={{ flexDirection: 'row', alignItems: 'center', gap: 6, marginBottom: 8 }}>
          <ClockIcon size={11} color={aura.accent} />
          <ScText style={{ fontSize: 10, fontWeight: '700', letterSpacing: 1.4, textTransform: 'uppercase', color: aura.accent }}>на {formatDuration(secs * 1000)}</ScText>
        </View>
      )}
      <View style={{ flexDirection: 'row', alignItems: 'flex-end', gap: 12 }}>
        <TextInput
          value={body}
          onChangeText={setBody}
          placeholder="Написать комментарий…"
          placeholderTextColor="rgba(255,255,255,0.2)"
          multiline
          onKeyPress={(e) => {
            // @ts-expect-error web-only nativeEvent shape
            if (e.nativeEvent.key === 'Enter' && !e.nativeEvent.shiftKey) { e.preventDefault?.(); void submit(); }
          }}
          style={{ flex: 1, minHeight: 38, maxHeight: 96, color: 'rgba(255,255,255,0.85)', fontSize: 13, lineHeight: 19, paddingTop: 8, ...({ outlineStyle: 'none' } as object) }}
        />
        <Pressable
          onPress={submit}
          disabled={!body.trim() || busy}
          style={{ width: 38, height: 38, borderRadius: 12, alignItems: 'center', justifyContent: 'center', backgroundColor: body.trim() ? aura.accentSoft : 'transparent', opacity: body.trim() ? 1 : 0.3 }}
        >
          {busy ? <ActivityIndicator size="small" color={aura.accent} /> : <SendIcon size={16} color={aura.accent} />}
        </Pressable>
      </View>
      {error && (
        <ScText numberOfLines={2} style={{ marginTop: 8, fontSize: 11, color: '#fca5a5' }}>{error}</ScText>
      )}
    </View>
  );
}
