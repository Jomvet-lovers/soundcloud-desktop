import { useState } from 'react';
import { Platform, Pressable, useWindowDimensions, View } from 'react-native';
import type { Comment, Track, User } from '@sc/data';
import { artUrl, Cover, CoverPlay, featuredGlass, GlassSurface, GradientText, MetaChip, ScText, TrackStatusBadges } from '@sc/ui';
import { type CreditGroup, participantGroups, uploadKindPill } from './format';
import { TrackActions } from './TrackActions';
import { TrackWaveFloor } from './TrackWaveFloor';
import type { TrackAura } from './useTrackAura';

/** Имя-ссылка с плавной подсветкой до белого (донор `hover:text-white`). */
function NameLink({ name, size, weight, dim, onPress }: { name: string; size: number; weight: '500' | '600'; dim: number; onPress: () => void }) {
  const [hover, setHover] = useState(false);
  return (
    <Pressable onPress={onPress} onHoverIn={() => setHover(true)} onHoverOut={() => setHover(false)}>
      <ScText style={{ fontSize: size, fontWeight: weight, color: hover ? '#fff' : `rgba(255,255,255,${dim})`, ...({ transition: 'color 0.25s ease' } as object) }}>{name}</ScText>
    </Pressable>
  );
}

/** Ряд соучастников: «feat. A, B · prod. C» — имена кликабельны, чуть крупнее. */
function CreditRow({ groups, align, onOpen }: { groups: CreditGroup[]; align: 'flex-start' | 'center'; onOpen: (id: string) => void }) {
  return (
    <View style={{ marginTop: 7, flexDirection: 'row', flexWrap: 'wrap', alignItems: 'center', justifyContent: align, gap: 3 }}>
      {groups.map((g, gi) => (
        <View key={gi} style={{ flexDirection: 'row', flexWrap: 'wrap', alignItems: 'center', gap: 3 }}>
          {gi > 0 && <ScText style={{ fontSize: 13, color: 'rgba(255,255,255,0.3)' }}>· </ScText>}
          {g.prefix ? <ScText style={{ fontSize: 13, color: 'rgba(255,255,255,0.45)' }}>{g.prefix}</ScText> : null}
          {g.people.map((p, pi) => (
            <View key={pi} style={{ flexDirection: 'row', alignItems: 'center' }}>
              {pi > 0 && <ScText style={{ fontSize: 13, color: 'rgba(255,255,255,0.45)' }}>, </ScText>}
              <NameLink name={p.name} size={13} weight="500" dim={0.55} onPress={() => p.id && onOpen(p.id)} />
            </View>
          ))}
          {g.suffix ? <ScText style={{ fontSize: 13, color: 'rgba(255,255,255,0.45)' }}>{g.suffix}</ScText> : null}
        </View>
      ))}
    </View>
  );
}

// ≥1024 обложка+инфо в ряд, уже — колонкой по центру
export function TrackHero({ track, artistUser, aura, isCurrent, isPlaying, comments, onPlay, onSeek, onOpenUser }: {
  track: Track;
  /** полный профиль артиста (для аватарки/галочки/счётчиков) */
  artistUser: User | null;
  aura: TrackAura;
  isCurrent: boolean;
  isPlaying: boolean;
  comments: Comment[];
  onPlay: () => void;
  onSeek: (seconds: number) => void;
  onOpenUser: (id: string) => void;
}) {
  const { width } = useWindowDimensions();
  const lg = width >= 1024;
  const md = width >= 768;
  const bg = artUrl(track.artwork_url, 't500x500');
  const artistId = track.uploader?.id ?? track.artist.id;
  const artistAvatar = track.artist.avatar_url ?? artistUser?.avatar_url ?? track.uploader?.avatar_url;
  const verified = track.artist_verified || artistUser?.verified || track.uploader?.verified || false;
  const coverSize = md ? 220 : 180;
  const titleSize = width >= 1280 ? 72 : md ? 60 : 36;
  const credits = participantGroups(track);
  const kind = uploadKindPill(track.upload_kind);

  return (
    <GlassSurface recipe={featuredGlass} style={{ borderRadius: 32 }}>
      <View pointerEvents="none" style={{ position: 'absolute', top: 0, left: 0, right: 0, bottom: 0, borderRadius: 32, overflow: 'hidden' }}>
        {Platform.OS === 'web' && bg && (
          <View
            pointerEvents="none"
            // @ts-expect-error web-only CSS
            style={{ position: 'absolute', top: 0, left: 0, right: 0, bottom: 0, backgroundImage: `url(${bg})`, backgroundSize: 'cover', backgroundPosition: 'center', opacity: 0.2, filter: 'blur(90px) saturate(1.4)', transform: [{ scale: 1.4 }] }}
          />
        )}
        <View pointerEvents="none" style={{ position: 'absolute', top: 0, left: 0, right: 0, bottom: 0, backgroundColor: 'rgba(10,10,12,0.5)' }} />
      </View>

      <View style={{ padding: md ? 32 : 24, gap: 28 }}>
        <View style={{ flexDirection: lg ? 'row' : 'column', alignItems: lg ? 'flex-start' : 'center', gap: lg ? 32 : 24 }}>
          <View style={{ padding: 6, borderRadius: 34, borderWidth: 2.5, borderColor: aura.accent, boxShadow: `0 22px 60px -14px ${aura.accentGlow}, 0 0 30px ${aura.accentGlow}` }}>
            <CoverPlay url={track.artwork_url} size={coverSize} radius={26} artSize="t500x500" isPlaying={isPlaying} playSize={64} onToggle={onPlay} />
          </View>

          <View style={{ flex: lg ? 1 : undefined, minWidth: 0, width: '100%', alignItems: lg ? 'flex-start' : 'center' }}>
            <View style={{ flexDirection: 'row', alignItems: 'center', gap: 8, flexWrap: 'wrap', justifyContent: lg ? 'flex-start' : 'center', marginBottom: 14 }}>
              <TrackStatusBadges storageState={track.badge?.storage_state} indexState={track.badge?.index_state} storageQuality={track.badge?.storage_quality} />
              {track.genre && <MetaChip label={track.genre} tint={aura.hasGenre ? aura.accent : undefined} />}
              {track.release_year != null && <MetaChip label={String(track.release_year)} />}
            </View>

            <GradientText
              text={track.title}
              gradient={aura.nameGradient}
              style={{
                fontSize: titleSize,
                fontWeight: '900',
                letterSpacing: titleSize * -0.05,
                lineHeight: titleSize * 0.95,
                textAlign: lg ? 'left' : 'center',
                ...({ filter: 'drop-shadow(0 6px 22px rgba(0,0,0,0.5))' } as object),
              }}
            />

            <View style={{ marginTop: 16, flexDirection: 'row', alignItems: 'center', gap: 10, flexWrap: 'wrap', justifyContent: lg ? 'flex-start' : 'center' }}>
              <Pressable onPress={() => onOpenUser(artistId)}>
                {artistAvatar && <Cover url={artistAvatar} size={28} radius={14} artSize="t200x200" />}
              </Pressable>
              <NameLink name={track.artist.name} size={15} weight="500" dim={0.75} onPress={() => onOpenUser(artistId)} />
              {verified && <ScText style={{ fontSize: 12, color: 'rgba(52,211,153,0.9)' }}>✓</ScText>}
              {kind && (
                <View style={{ paddingHorizontal: 7, paddingVertical: 2.5, borderRadius: 6, backgroundColor: `${kind.tint}22` }}>
                  <ScText style={{ fontSize: 9, fontWeight: '800', letterSpacing: 1, color: kind.tint }}>{kind.label}</ScText>
                </View>
              )}
            </View>

            {credits.length > 0 && <CreditRow groups={credits} align={lg ? 'flex-start' : 'center'} onOpen={onOpenUser} />}

            <View style={{ marginTop: 24, alignItems: lg ? 'flex-start' : 'center', width: '100%' }}>
              <TrackActions track={track} isPlaying={isPlaying} onPlay={onPlay} />
            </View>
          </View>
        </View>

        <View style={{ borderTopWidth: 1, borderTopColor: 'rgba(255,255,255,0.07)', paddingTop: 24 }}>
          <TrackWaveFloor track={track} isCurrent={isCurrent} comments={comments} aura={aura} onSeek={onSeek} />
        </View>
      </View>
    </GlassSurface>
  );
}
