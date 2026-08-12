import { useState } from 'react';
import { Pressable, View } from 'react-native';
import type { Track } from '@sc/data';
import { ChevronDownIcon, ChevronUpIcon, GlassSurface, HashIcon, ScText, StatOrb, surfaceGlass } from '@sc/ui';
import { useLiked } from '../../player/likes';
import { formatCount, likeCountOf } from './format';
import type { TrackAura } from './useTrackAura';

interface LinerNotesProps {
  track: Track;
  aura: TrackAura;
  commentCount: number;
}

/** Дата релиза из ISO (донор `dateFormatted`); фолбэк, когда нет release_year. */
function releaseDate(iso: string | null): string | null {
  if (!iso) return null;
  const ts = Date.parse(iso);
  if (!Number.isFinite(ts)) return null;
  return new Date(ts).toLocaleDateString('ru-RU', { day: 'numeric', month: 'short', year: 'numeric' });
}

/** Кредит-как-типографика: лейбл сверху, значение снизу. Колонки ~1/3 (донор grid). */
function Credit({ label, value }: { label: string; value: string }) {
  return (
    <View style={{ flexBasis: '30%', flexGrow: 0, minWidth: 130, gap: 5 }}>
      <ScText style={{ fontSize: 10, fontWeight: '700', textTransform: 'uppercase', letterSpacing: 2.2, color: 'rgba(255,255,255,0.3)' }}>
        {label}
      </ScText>
      <ScText numberOfLines={1} style={{ fontSize: 13, color: 'rgba(255,255,255,0.7)' }}>
        {value}
      </ScText>
    </View>
  );
}

export function LinerNotes({ track, aura, commentCount }: LinerNotesProps) {
  const [expanded, setExpanded] = useState(false);
  const liked = useLiked(track.id, track.user_favorite ?? false);

  const desc = track.description?.trim() || null;
  const descLong = !!desc && desc.length > 280;

  const released = releaseDate(track.release_date) ?? (track.release_year != null ? String(track.release_year) : releaseDate(track.created_at));
  const credits: { label: string; value: string }[] = [];
  if (track.album?.title) credits.push({ label: 'АЛЬБОМ', value: track.album.title });
  if (released) credits.push({ label: 'РЕЛИЗ', value: released });
  if (track.language) credits.push({ label: 'ЯЗЫК', value: track.language });
  if (track.isrc) credits.push({ label: 'ISRC', value: track.isrc });

  return (
    <GlassSurface recipe={surfaceGlass} style={{ borderRadius: 32, padding: 26, gap: 24 }}>
      <View style={{ flexDirection: 'row', flexWrap: 'wrap', gap: 14 }}>
        {/* донор показывает плашки всегда — при отсутствии данных «—», а не пустоту */}
        <StatOrb value={formatCount(track.play_count)} label="ПРОСЛУШИВАНИЯ" glow={aura.accentGlow} />
        <StatOrb value={formatCount(likeCountOf(track, liked))} label="ЛАЙКИ" glow={aura.accentGlow} />
        <StatOrb value={formatCount(track.reposts_count)} label="РЕПОСТЫ" glow={aura.accentGlow} />
        <StatOrb value={formatCount(commentCount)} label="КОММЕНТАРИИ" glow={aura.accentGlow} />
      </View>

      {desc && (
        <View>
          <ScText
            style={{
              fontSize: 10,
              fontWeight: '700',
              textTransform: 'uppercase',
              letterSpacing: 2.2,
              color: 'rgba(255,255,255,0.3)',
              marginBottom: 10,
            }}
          >
            ОПИСАНИЕ
          </ScText>
          <ScText
            numberOfLines={!expanded && descLong ? 4 : undefined}
            style={{ fontSize: 13.5, lineHeight: 20, color: 'rgba(255,255,255,0.55)' }}
          >
            {desc}
          </ScText>
          {descLong && (
            <Pressable
              onPress={() => setExpanded((v) => !v)}
              style={{ flexDirection: 'row', alignItems: 'center', gap: 4, marginTop: 8 }}
            >
              {expanded ? (
                <ChevronUpIcon size={13} color="rgba(255,255,255,0.35)" />
              ) : (
                <ChevronDownIcon size={13} color="rgba(255,255,255,0.35)" />
              )}
              <ScText style={{ fontSize: 11, color: 'rgba(255,255,255,0.35)' }}>
                {expanded ? 'Показать меньше' : 'Показать ещё'}
              </ScText>
            </Pressable>
          )}
        </View>
      )}

      {credits.length > 0 && (
        <View style={{ flexDirection: 'row', flexWrap: 'wrap', columnGap: 32, rowGap: 16 }}>
          {credits.map((c) => (
            <Credit key={c.label} label={c.label} value={c.value} />
          ))}
        </View>
      )}

      {track.tags.length > 0 && (
        <View style={{ flexDirection: 'row', alignItems: 'center', flexWrap: 'wrap', gap: 6, paddingTop: 4 }}>
          <HashIcon size={12} color="rgba(255,255,255,0.2)" />
          {track.tags.map((tag) => (
            <View
              key={tag}
              style={{
                paddingHorizontal: 10,
                paddingVertical: 4,
                borderRadius: 999,
                backgroundColor: 'rgba(255,255,255,0.04)',
                borderWidth: 1,
                borderColor: 'rgba(255,255,255,0.05)',
              }}
            >
              <ScText style={{ fontSize: 10, fontWeight: '500', color: 'rgba(255,255,255,0.4)' }}>{tag}</ScText>
            </View>
          ))}
        </View>
      )}
    </GlassSurface>
  );
}
