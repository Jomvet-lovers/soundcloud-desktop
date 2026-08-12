import { ActivityIndicator, View } from 'react-native';
import type { Comment } from '@sc/data';
import { formatAgo, formatCount, MessageCircleIcon, ScText, SectionHeading, VoiceCard } from '@sc/ui';
import { CommentComposer } from './CommentComposer';
import type { TrackAura } from './useTrackAura';

export function TrackVoices({ trackUrn, comments, commentCount, loading, isCurrent, aura, onSeek, onPosted }: {
  trackUrn: string;
  comments: Comment[];
  commentCount: number;
  loading: boolean;
  isCurrent: boolean;
  aura: TrackAura;
  onSeek: (seconds: number) => void;
  onPosted: (comment: Comment) => void;
}) {
  return (
    <View style={{ gap: 20 }}>
      <SectionHeading icon={<MessageCircleIcon size={14} color={aura.accent} />} title="Голоса слушателей" count={formatCount(commentCount)} accentSoft={aura.accentSoft} />

      <CommentComposer trackUrn={trackUrn} isCurrent={isCurrent} aura={aura} onPosted={onPosted} />

      {loading ? (
        <View style={{ paddingVertical: 40, alignItems: 'center' }}>
          <ActivityIndicator color="rgba(255,255,255,0.2)" />
        </View>
      ) : comments.length === 0 ? (
        <View style={{ paddingVertical: 64, alignItems: 'center', gap: 16 }}>
          <View style={{ width: 64, height: 64, borderRadius: 16, alignItems: 'center', justifyContent: 'center', backgroundColor: 'rgba(255,255,255,0.03)', borderWidth: 1, borderColor: 'rgba(255,255,255,0.06)' }}>
            <MessageCircleIcon size={24} color="rgba(255,255,255,0.15)" />
          </View>
          <ScText style={{ fontSize: 14, color: 'rgba(255,255,255,0.3)' }}>Пока тихо. Стань первым голосом</ScText>
        </View>
      ) : (
        <View style={{ gap: 12 }}>
          {comments.map((c) => (
            <VoiceCard key={c.id} avatarUrl={c.user.avatar_url} username={c.user.username} timestampMs={c.timestamp_ms} ago={formatAgo(c.created_at)} body={c.body} accent={aura.accent} accentSoft={aura.accentSoft} accentGlow={aura.accentGlow} onSeek={onSeek} />
          ))}
        </View>
      )}
    </View>
  );
}
