import { useEffect, useMemo, useRef, useState } from 'react';
import { ActivityIndicator, Platform, Pressable, ScrollView, View } from 'react-native';
import type { Comment } from '@sc/data';
import { Cover, formatDuration, MessageCircleIcon, ScText, useScTheme } from '@sc/ui';
import { usePlayerState } from '../../player/PlayerContext';
import { useWholeSeconds } from '../../player/position';
import type { CommentsLoad } from './useComments';

/** Тайм-комменты (донор `TimedComments`): караоке-лента, отсортированная по таймкоду.
 *  Активный = ближайший «прошедший» (бинпоиск по позиции) — подсвечен и доскроллен в
 *  центр; клик — seek к таймкоду. Позиция через `useWholeSeconds()` (1Гц) — ре-рендер
 *  только этой ленты. Даль от фокуса гасится масштабом/прозрачностью (глубина). */
export function CommentsPane({ loading, items }: CommentsLoad) {
  const { accent } = useScTheme();
  const player = usePlayerState();
  const secs = useWholeSeconds();
  const scrollRef = useRef<ScrollView>(null);
  const offsets = useRef(new Map<number, { y: number; h: number }>());
  const [viewH, setViewH] = useState(0);

  const timed = useMemo(
    () =>
      items
        .filter((c) => c.timestamp_ms != null && c.body.trim())
        .sort((a, b) => (a.timestamp_ms ?? 0) - (b.timestamp_ms ?? 0)),
    [items],
  );

  const activeIndex = useMemo(() => {
    const posMs = Math.max(0, secs) * 1000;
    let lo = 0;
    let hi = timed.length - 1;
    let best = -1;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      if ((timed[mid].timestamp_ms ?? 0) <= posMs) {
        best = mid;
        lo = mid + 1;
      } else {
        hi = mid - 1;
      }
    }
    return best;
  }, [timed, secs]);

  const focus = activeIndex >= 0 ? activeIndex : timed.length > 0 ? 0 : -1;

  useEffect(() => {
    if (focus < 0 || !viewH) return;
    const o = offsets.current.get(timed[focus]?.id ?? -1);
    if (o) scrollRef.current?.scrollTo({ y: Math.max(0, o.y - viewH / 2 + o.h / 2), animated: true });
  }, [focus, timed, viewH]);

  if (loading && timed.length === 0) {
    return (
      <View style={{ flex: 1, alignItems: 'center', justifyContent: 'center', gap: 12 }}>
        <ActivityIndicator color="rgba(255,255,255,0.4)" />
        <ScText style={{ fontSize: 13, color: 'rgba(255,255,255,0.25)' }}>Комментарии…</ScText>
      </View>
    );
  }
  if (timed.length === 0) {
    return (
      <View style={{ flex: 1, alignItems: 'center', justifyContent: 'center', gap: 14, paddingHorizontal: 48 }}>
        <MessageCircleIcon size={40} color="rgba(255,255,255,0.06)" />
        <ScText style={{ fontSize: 15, fontWeight: '600', color: 'rgba(255,255,255,0.3)' }}>Пока нет комментариев</ScText>
      </View>
    );
  }

  const maskStyle =
    Platform.OS === 'web'
      ? ({ maskImage: 'linear-gradient(transparent 0%, black 12%, black 88%, transparent 100%)', WebkitMaskImage: 'linear-gradient(transparent 0%, black 12%, black 88%, transparent 100%)' } as object)
      : null;

  return (
    <ScrollView
      ref={scrollRef}
      onLayout={(e) => setViewH(e.nativeEvent.layout.height)}
      showsVerticalScrollIndicator={false}
      style={maskStyle}
      contentContainerStyle={{ paddingVertical: viewH * 0.4, paddingHorizontal: 24, gap: 10 }}
    >
      {timed.map((c, i) => {
        const state = i < activeIndex ? 'past' : i === activeIndex ? 'active' : 'future';
        const dist = Math.abs(i - focus);
        const scale = Math.max(0.9, 1 - dist * 0.035);
        const opacity = state === 'active' ? 1 : Math.max(0.28, dist === 0 ? 0.94 : 1 - dist * 0.15);
        return (
          <View
            key={c.id}
            onLayout={(e) => offsets.current.set(c.id, { y: e.nativeEvent.layout.y, h: e.nativeEvent.layout.height })}
            style={{ opacity, transform: [{ scale }] }}
          >
            <CommentCard comment={c} state={state} accent={accent.base} onSeek={() => player.seek((c.timestamp_ms ?? 0) / 1000)} />
          </View>
        );
      })}
    </ScrollView>
  );
}

function CommentCard({
  comment,
  state,
  accent,
  onSeek,
}: {
  comment: Comment;
  state: 'past' | 'active' | 'future';
  accent: string;
  onSeek: () => void;
}) {
  const bg = state === 'active' ? 'rgba(255,255,255,0.12)' : state === 'past' ? 'rgba(255,255,255,0.025)' : 'rgba(255,255,255,0.045)';
  return (
    <Pressable
      onPress={onSeek}
      style={{
        borderRadius: 16,
        borderWidth: 1,
        paddingHorizontal: 14,
        paddingVertical: 11,
        backgroundColor: bg,
        borderColor: state === 'active' ? `${accent}40` : 'rgba(255,255,255,0.05)',
        boxShadow: state === 'active' ? '0 16px 36px rgba(0,0,0,0.26)' : undefined,
      }}
    >
      <View style={{ flexDirection: 'row', gap: 12, alignItems: 'flex-start' }}>
        <Cover url={comment.user.avatar_url} size={36} radius={18} artSize="t67x67" />
        <View style={{ flex: 1, minWidth: 0 }}>
          <View style={{ flexDirection: 'row', alignItems: 'center', gap: 8 }}>
            <ScText numberOfLines={1} style={{ flex: 1, fontSize: 12, fontWeight: '600', color: 'rgba(255,255,255,0.78)' }}>
              {comment.user.username}
            </ScText>
            <ScText style={{ fontSize: 10, fontWeight: '600', color: state === 'active' ? accent : 'rgba(255,255,255,0.3)' }}>
              {formatDuration(comment.timestamp_ms ?? 0)}
            </ScText>
          </View>
          <ScText style={{ marginTop: 3, fontSize: 13, lineHeight: 18, color: 'rgba(255,255,255,0.6)' }}>{comment.body}</ScText>
        </View>
      </View>
    </Pressable>
  );
}
