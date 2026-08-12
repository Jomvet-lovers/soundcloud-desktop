import { useEffect, useRef, useState, type ReactNode } from 'react';
import { View } from 'react-native';
import { type Track, useSc } from '@sc/data';
import { ScText, Shelf, Skeleton, TrackCard } from '@sc/ui';
import { useLikedTracks } from '../../player/likes';
import type { PlayerState } from '../../player/PlayerContext';
import { useCardProps } from './stations';

const SHELF_CAP = 24;
const REC_CANDIDATES = 24;
const CARD_WIDTH = 176;

function ShelfHeader({ index, label }: { index: string; label: string }) {
  return (
    <View style={{ flexDirection: 'row', alignItems: 'center', gap: 10, marginBottom: 12, paddingLeft: 4 }}>
      <ScText token="counter" style={{ fontSize: 10.5, color: 'rgba(255,255,255,0.25)' }}>
        {index}
      </ScText>
      <ScText
        style={{
          fontSize: 12,
          fontWeight: '600',
          textTransform: 'uppercase',
          letterSpacing: 12 * 0.14,
          color: 'rgba(255,255,255,0.55)',
        }}
      >
        {label}
      </ScText>
      <View style={{ flex: 1, height: 1, backgroundColor: 'rgba(255,255,255,0.05)' }} />
    </View>
  );
}

function ShelfSkeletonRow({ count = 8 }: { count?: number }) {
  return (
    <>
      {Array.from({ length: count }).map((_, i) => (
        <View key={i} style={{ width: CARD_WIDTH }}>
          <Skeleton style={{ width: '100%', aspectRatio: 1 }} radius={16} />
          <Skeleton style={{ marginTop: 10, height: 14, width: '75%' }} radius={4} />
          <Skeleton style={{ marginTop: 6, height: 11, width: '50%' }} radius={4} />
        </View>
      ))}
    </>
  );
}

function SubShelf({
  index,
  label,
  loading,
  children,
}: {
  index: string;
  label: string;
  loading: boolean;
  children: ReactNode;
}) {
  return (
    <View>
      <ShelfHeader index={index} label={label} />
      <Shelf>{loading ? <ShelfSkeletonRow /> : children}</Shelf>
    </View>
  );
}

/** «Затоны» — лайкнутое и рекомендации вне основной реки, тихой водой. */
export function ArchiveStation({ liked, player }: { liked: Track[]; player: PlayerState }) {
  const sc = useSc();
  const likedRef = useRef(liked);
  likedRef.current = liked;

  const [recommended, setRecommended] = useState<Track[]>([]);
  const [recLoading, setRecLoading] = useState(false);
  const card = useCardProps(player);
  // Полка «Любимые» — из общего стора лайков (реактивно: анлайк убирает карточку).
  const likedTracks = useLikedTracks();

  const seedId = liked[0]?.id;

  useEffect(() => {
    if (!seedId) {
      setRecommended([]);
      return;
    }
    let alive = true;
    setRecLoading(true);
    (async () => {
      const clusters = await sc.home.similar(seedId, 20);
      const ids = [...new Set(clusters.flatMap((c) => c.track_ids))].slice(0, REC_CANDIDATES);
      const tracks = ids.length ? await sc.tracks.resolveMany(ids) : [];
      if (!alive) return;
      const likedIds = new Set(likedRef.current.map((t) => t.id));
      setRecommended(tracks.filter((t) => !likedIds.has(t.id)));
    })()
      .catch(() => alive && setRecommended([]))
      .finally(() => alive && setRecLoading(false));
    return () => {
      alive = false;
    };
  }, [sc, seedId]);

  if (likedTracks.length === 0 && recommended.length === 0 && !recLoading) return null;

  const likedItems = likedTracks.slice(0, SHELF_CAP);
  const recItems = recommended.slice(0, SHELF_CAP);

  return (
    <View style={{ paddingTop: 56 }}>
      <View style={{ marginBottom: 20 }}>
        <ScText style={{ fontSize: 22, fontWeight: '700', lineHeight: 27.5, letterSpacing: -0.33, color: 'rgba(255,255,255,0.92)' }}>
          Затоны
        </ScText>
        <ScText style={{ marginTop: 4, fontSize: 13, lineHeight: 17.875, color: 'rgba(255,255,255,0.5)' }}>
          лайкнутое и подборка под тебя — тихая вода вне течения
        </ScText>
      </View>

      <View style={{ gap: 32 }}>
        {likedTracks.length > 0 && (
          <SubShelf index="01" label="Любимые треки" loading={false}>
            {likedItems.map((t) => (
              <View key={t.id} style={{ width: CARD_WIDTH }}>
                {/* очередь = ВЕСЬ список лайков (донор-паритет: старт строит likes-queue) */}
                <TrackCard {...card(t, likedTracks)} />
              </View>
            ))}
          </SubShelf>
        )}

        {likedTracks.length > 0 && (recLoading || recItems.length > 0) && (
          <SubShelf index="02" label="Рекомендации для вас" loading={recLoading}>
            {recItems.map((t) => (
              <View key={t.id} style={{ width: CARD_WIDTH }}>
                <TrackCard {...card(t, recItems)} />
              </View>
            ))}
          </SubShelf>
        )}
      </View>
    </View>
  );
}
