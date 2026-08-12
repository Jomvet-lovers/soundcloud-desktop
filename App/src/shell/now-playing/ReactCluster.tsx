import { useCallback, useEffect, useState } from 'react';
import { View } from 'react-native';
import type { Track } from '@sc/data';
import { useSc } from '@sc/data';
import { HeartIcon, ScText, ThumbsDownIcon, useScTheme } from '@sc/ui';
import { useT } from '../../i18n';
import { toggleLike, useLiked } from '../../player/likes';
import { scId, usePlayerState } from '../../player/PlayerContext';
import { IconButton } from './IconButton';

/** Лайк/дизлайк/качество текущего трека (донор `ReactCluster`) — оптимистично.
 *  Дизлайк текущего играющего снимает лайк и перескакивает трек (донор). */
export function ReactCluster({ track }: { track: Track | null }) {
  const sc = useSc();
  const player = usePlayerState();
  const { accent } = useScTheme();
  const t = useT();
  const [disliked, setDisliked] = useState(false);
  const liked = useLiked(track?.id ?? '', track?.user_favorite ?? false);

  // Статус дизлайка для нового трека (наша БД — истина).
  useEffect(() => {
    if (!track) return;
    let alive = true;
    setDisliked(false);
    void sc.tracks.dislikeStatus(scId(track)).then((d) => { if (alive) setDisliked(d); }).catch(() => {});
    return () => { alive = false; };
  }, [track, sc]);

  const onLike = useCallback(() => {
    if (track) toggleLike(track, sc);
  }, [track, sc]);

  const toggleDislike = useCallback(() => {
    if (!track) return;
    const next = !disliked;
    setDisliked(next);
    if (next && liked) toggleLike(track, sc); // дизлайк снимает лайк
    const call = next ? sc.tracks.dislike(scId(track)) : sc.tracks.undislike(scId(track));
    void call.catch(() => setDisliked(!next));
    if (next && player.currentTrack?.id === track.id) player.next();
  }, [track, disliked, liked, sc, player]);

  if (!track) return null;

  return (
    <View style={{ flexDirection: 'row', alignItems: 'center', gap: 1 }}>
      <IconButton size={36} onPress={onLike} tooltip={t('player.like')}>
        <HeartIcon size={16} color={liked ? accent.base : 'rgba(255,255,255,0.3)'} filled={liked} />
      </IconButton>
      <IconButton size={36} onPress={toggleDislike} tooltip={t('player.dislike')}>
        <ThumbsDownIcon size={16} color={disliked ? '#fb7185' : 'rgba(255,255,255,0.3)'} filled={disliked} />
      </IconButton>
      <QualityBadge track={track} />
    </View>
  );
}

/** Бейдж качества/источника (донор `PlaybackQualityBadge`) из `_scd_meta`:
 *  HQ/SQ по storage_quality + «CDN» когда трек лежит в нашем хранилище. */
function QualityBadge({ track }: { track: Track }) {
  const quality = track.badge?.storage_quality;
  const cdn = track.badge?.storage_state === 'ok';
  if (!quality) return null;
  const isHq = quality === 'hq';

  return (
    <View style={{ flexDirection: 'row', alignItems: 'center', gap: 6, marginLeft: 4 }}>
      <View
        style={{
          height: 22, borderRadius: 7, paddingHorizontal: 7, alignItems: 'center', justifyContent: 'center',
          borderWidth: 1,
          borderColor: isHq ? 'rgba(255,255,255,0.14)' : 'rgba(255,255,255,0.08)',
          backgroundColor: isHq ? 'rgba(255,255,255,0.08)' : 'rgba(255,255,255,0.04)',
        }}
      >
        <ScText style={{ fontSize: 9, fontWeight: '700', letterSpacing: 1.4, color: isHq ? 'rgba(255,255,255,0.92)' : 'rgba(255,255,255,0.68)' }}>
          {isHq ? 'HQ' : 'SQ'}
        </ScText>
      </View>
      {cdn && (
        <View style={{ flexDirection: 'row', alignItems: 'center', gap: 5, height: 22, borderRadius: 7, paddingHorizontal: 7, borderWidth: 1, borderColor: 'rgba(183,255,216,0.16)', backgroundColor: 'rgba(183,255,216,0.07)' }}>
          <View style={{ width: 6, height: 6, borderRadius: 3, backgroundColor: '#b7ffd8', boxShadow: '0 0 8px rgba(183,255,216,0.55)' }} />
          <ScText style={{ fontSize: 8, fontWeight: '600', letterSpacing: 1.2, color: 'rgba(223,247,233,0.82)' }}>CDN</ScText>
        </View>
      )}
    </View>
  );
}
