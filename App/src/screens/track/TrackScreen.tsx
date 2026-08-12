import { useEffect, useState } from 'react';
import { ActivityIndicator, Pressable, ScrollView, useWindowDimensions, View } from 'react-native';
import { type Comment, type Track, type Urn, type User, useSc } from '@sc/data';
import { ChevronLeftIcon, ScText } from '@sc/ui';
import { go } from '../../nav/nav-bus';
import { setAmbient } from '../../player/ambient';
import { usePlayerState } from '../../player/PlayerContext';
import { LinerNotes } from './LinerNotes';
import { SimilarRoom } from './SimilarRoom';
import { TrackHero } from './TrackHero';
import { TrackSleeve } from './TrackSleeve';
import { TrackVoices } from './TrackVoices';
import { useTrackAura } from './useTrackAura';

export function TrackScreen({ urn, onBack }: { urn: string; onBack: () => void }) {
  const sc = useSc();
  const player = usePlayerState();
  const [track, setTrack] = useState<Track | null>(null);
  const [comments, setComments] = useState<Comment[]>([]);
  const [commentsLoading, setCommentsLoading] = useState(true);
  const [related, setRelated] = useState<Track[]>([]);
  const [relatedLoading, setRelatedLoading] = useState(true);
  const [favoriters, setFavoriters] = useState<User[]>([]);
  const [uploader, setUploader] = useState<User | null>(null);

  useEffect(() => {
    let alive = true;
    setTrack(null);
    setUploader(null);
    void sc.tracks.resolveMany([urn]).then(([t]) => {
      if (!alive || !t) return;
      setTrack(t);
      const artistId = t.uploader?.id ?? t.artist.id;
      if (artistId) void sc.users.byUrn(artistId as Urn).then((u) => { if (alive) setUploader(u); }).catch(() => {});
    }).catch(() => {});
    return () => { alive = false; };
  }, [sc, urn]);

  useEffect(() => {
    let alive = true;
    setCommentsLoading(true);
    setRelatedLoading(true);
    void sc.tracks.comments(urn as Urn, 60, 0).then((r) => { if (alive) { setComments(r.items); setCommentsLoading(false); } }).catch(() => alive && setCommentsLoading(false));
    void sc.tracks.related(urn as Urn, 10).then(async (r) => {
      let full = r.items;
      try {
        const res = await sc.tracks.resolveMany(r.items.map((t) => t.id));
        if (res.length) full = res;
      } catch { /* оставляем сырые */ }
      if (alive) { setRelated(full); setRelatedLoading(false); }
    }).catch(() => alive && setRelatedLoading(false));
    void sc.tracks.favoriters(urn as Urn, 12).then((r) => { if (alive) setFavoriters(r.items); }).catch(() => {});
    return () => { alive = false; };
  }, [sc, urn]);

  const { width } = useWindowDimensions();
  const wide = width >= 1024;
  const aura = useTrackAura(track?.genre);

  const isCurrent = !!track && player.currentTrack?.id === track.id;
  const isPlaying = isCurrent && player.playing;

  useEffect(() => {
    setAmbient({ colors: [aura.accent], energy: isPlaying ? Math.min(1, aura.energy + 0.12) : aura.energy });
    return () => setAmbient(null);
  }, [aura.accent, aura.energy, isPlaying]);

  const onPlay = () => { if (track) player.toggle(track, [track]); };
  const onSeek = (s: number) => {
    if (!track) return;
    if (isCurrent) player.seek(s);
    else player.toggle(track, [track], s);
  };

  return (
    <ScrollView style={{ flex: 1 }} contentContainerStyle={{ minHeight: '100%' as never }} showsVerticalScrollIndicator={false}>
      <View style={{ width: '100%', maxWidth: 1320, alignSelf: 'center', paddingHorizontal: width >= 768 ? 32 : 16, paddingTop: 20, paddingBottom: 40, gap: 28 }}>
        <View style={{ flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between' }}>
          <Pressable onPress={onBack} style={{ width: 36, height: 36, borderRadius: 18, alignItems: 'center', justifyContent: 'center' }}>
            <ChevronLeftIcon size={18} color="rgba(255,255,255,0.55)" />
          </Pressable>
          {track?.genre && (
            <ScText style={{ fontSize: 10, letterSpacing: 3, textTransform: 'uppercase', color: 'rgba(255,255,255,0.2)' }}>Комната под этот трек</ScText>
          )}
        </View>

        {!track ? (
          <View style={{ alignItems: 'center', paddingTop: 120, gap: 12 }}>
            <ActivityIndicator color="rgba(255,255,255,0.25)" />
            <ScText level="tertiary">Открываем комнату…</ScText>
          </View>
        ) : (
          <>
            <TrackHero
              track={track}
              artistUser={uploader}
              aura={aura}
              isCurrent={isCurrent}
              isPlaying={isPlaying}
              comments={comments}
              onPlay={onPlay}
              onSeek={onSeek}
              onOpenUser={(id) => go({ name: 'track', urn: id })}
            />

            <LinerNotes track={track} aura={aura} commentCount={track.comment_count ?? comments.length} />

            <SimilarRoom trackUrn={track.id} />

            <View style={{ flexDirection: wide ? 'row' : 'column', gap: wide ? 32 : 28, alignItems: 'flex-start' }}>
              <View style={{ flex: wide ? 1 : undefined, width: '100%', minWidth: 0 }}>
                <TrackVoices trackUrn={track.id} comments={comments} commentCount={track.comment_count ?? comments.length} loading={commentsLoading} isCurrent={isCurrent} aura={aura} onSeek={onSeek} onPosted={(c) => setComments((prev) => [c, ...prev])} />
              </View>
              <View style={{ width: wide ? 340 : '100%' }}>
                <TrackSleeve
                  track={track}
                  uploader={uploader}
                  favoriters={favoriters}
                  related={related}
                  relatedLoading={relatedLoading}
                  aura={aura}
                  currentId={player.currentTrack?.id ?? null}
                  playing={player.playing}
                  onOpenUser={(id) => go({ name: 'track', urn: id })}
                  onPlayRelated={(rt, queue) => player.toggle(rt, queue)}
                />
              </View>
            </View>
          </>
        )}
      </View>
    </ScrollView>
  );
}
