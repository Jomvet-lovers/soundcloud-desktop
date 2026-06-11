import React, {useMemo} from 'react';
import {art} from '../../../lib/formatters';
import {playWhite14} from '../../../lib/icons';
import {ClusterFeedbackProvider} from '../../../lib/recsFeedback';
import type {Track} from '../../../stores/player';
import {usePlayerStore} from '../../../stores/player';
import type {ClusterHydrated, ClusterNeighborDto} from '../../music/cluster';

/** «Твои артисты» — круги на частотном проводе. Клик играет очередь из лучших
 *  треков артиста (resolveQueue — тот же контракт, что у NeighborCard). */
export const ArtistWire = React.memo(function ArtistWire({
  cluster,
  resolveQueue,
}: {
  cluster: ClusterHydrated;
  resolveQueue: (track: Track) => Promise<Track[]>;
}) {
  const ctx = useMemo(() => ({ clusterId: String(cluster.id) }), [cluster.id]);

  const pairs = useMemo(() => {
    const byId = new Map<string, Track>();
    for (const t of cluster.tracks) {
      const id = t.urn.split(':').pop();
      if (id) byId.set(id, t);
    }
    const out: Array<{ neighbor: ClusterNeighborDto; track: Track }> = [];
    for (const n of cluster.neighbors ?? []) {
      const track = byId.get(String(n.track_id));
      if (track) out.push({ neighbor: n, track });
    }
    return out;
  }, [cluster]);

  if (pairs.length === 0) return null;

  const play = async (track: Track) => {
    const queue = await resolveQueue(track);
    usePlayerStore.getState().play(queue[0] ?? track, queue);
  };

  return (
    <ClusterFeedbackProvider value={ctx}>
      <div className="relative pb-1 pt-2">
        <span
          className="pointer-events-none absolute left-0 right-0 top-[58px] h-px"
          style={{
            background:
              'linear-gradient(90deg, transparent, rgba(255,255,255,0.2) 8%, rgba(255,255,255,0.2) 92%, transparent)',
          }}
        />
        <div
          className="relative flex items-start gap-7 overflow-x-auto px-1 pb-2"
          style={{ scrollbarWidth: 'none' }}
        >
          {pairs.map(({ neighbor, track }) => (
            <button
              key={neighbor.artist_id}
              type="button"
              onClick={() => void play(track)}
              className="group flex w-[96px] flex-none cursor-pointer flex-col items-center gap-2.5 transition-transform hover:-translate-y-1"
              title={neighbor.artist_name}
            >
              <span className="relative block size-[92px] overflow-hidden rounded-full shadow-[0_0_0_1px_rgba(255,255,255,0.14),0_12px_28px_rgba(0,0,0,0.45)] transition-shadow group-hover:shadow-[0_0_0_2px_var(--color-accent),0_0_24px_var(--color-accent-glow),0_12px_28px_rgba(0,0,0,0.45)]">
                {neighbor.avatar_url ? (
                  <img
                    src={art(neighbor.avatar_url, 't300x300') ?? undefined}
                    alt=""
                    className="size-full object-cover"
                    decoding="async"
                    loading="lazy"
                  />
                ) : (
                  <span className="block size-full bg-white/[0.06]" />
                )}
                <span className="absolute inset-0 flex items-center justify-center rounded-full bg-black/40 text-white opacity-0 transition-opacity group-hover:opacity-100">
                  {playWhite14}
                </span>
              </span>
              <span className="max-w-full truncate text-[13px] font-semibold text-white/85">
                {neighbor.artist_name}
              </span>
            </button>
          ))}
        </div>
      </div>
    </ClusterFeedbackProvider>
  );
});
