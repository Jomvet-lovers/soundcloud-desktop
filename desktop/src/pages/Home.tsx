import {useMemo, useState} from 'react';
import {ArchiveStation} from '../components/home/ether/ArchiveStation';
import {EtherMasthead} from '../components/home/ether/EtherMasthead';
import {EtherWave} from '../components/home/ether/EtherWave';
import {ETHER_KEYFRAMES} from '../components/home/ether/keyframes';
import {WaveFrame} from '../components/home/WaveFrame';
import {useSoundprint} from '../components/library/useSoundprint';
import {SoundWaveLockOverlay} from '../components/music/soundwave';
import {useLikedTracks} from '../lib/hooks';
import {useAuthStore} from '../stores/auth';

/** Главная — «Эфир»: личная радиостанция. Masthead с позывными и спектром,
 *  sticky шкала-тюнер, on-air дека и станции-кластеры; внизу — архив эфира. */
export function Home() {
  const user = useAuthStore((s) => s.user);
  const likedTracksQuery = useLikedTracks(100);

  // Выбранный жанр спектра ретинтит всю страницу (атмосфера + шапка).
  const [genre, setGenre] = useState<string | null>(null);
  const sound = useSoundprint(likedTracksQuery.tracks, genre);

  const likedShelfTracks = useMemo(
    () => likedTracksQuery.tracks.slice(0, 50),
    [likedTracksQuery.tracks],
  );

  return (
    <WaveFrame sound={sound}>
      <style>{ETHER_KEYFRAMES}</style>
      {user && <EtherMasthead user={user} sound={sound} selected={genre} onSelect={setGenre} />}

      <div className="relative">
        <EtherWave />
        <SoundWaveLockOverlay />
      </div>

      <ArchiveStation likedTracks={likedShelfTracks} likedLoading={likedTracksQuery.isLoading} />
    </WaveFrame>
  );
}
