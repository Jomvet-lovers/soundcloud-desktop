import {memo} from 'react';
import {useTranslation} from 'react-i18next';
import {type Aura, auraRgb, auraRgba} from '../../lib/aura';
import {art} from '../../lib/formatters';
import {User as UserIcon} from '../../lib/icons';
import {usePerfMode} from '../../lib/perf';
import type {Track} from '../../stores/player';
import {ArtworkMosaic} from '../library/ArtworkMosaic';
import {SoundprintBars} from '../library/SoundprintBars';
import type {Soundprint} from '../library/useSoundprint';
import {VibePortal} from '../music/soundwave/vibe-portal';
import {HERO_STAR_SEEDS, StarField} from '../user/StarField';

interface MastheadUser {
    username: string;
    avatar_url: string;
}

function greetingKey(): string {
    const h = new Date().getHours();
    if (h < 5) return 'library.greetNight';
    if (h < 12) return 'library.greetMorning';
    if (h < 18) return 'library.greetDay';
    return 'library.greetEvening';
}

/** Idle "now playing" equalizer flourish — signals the room is alive. */
const EqBars = memo(function EqBars({color}: { color: string }) {
    const perf = usePerfMode();
    if (!perf.idleAnim) return null;
    const delays = ['-0.2s', '-0.56s', '-0.08s', '-0.38s'];
    return (
        <span className="inline-flex items-end gap-[2px] h-[9px]">
      {delays.map((d) => (
          <i
              key={d}
              className="w-[2px] rounded-full"
              style={{
                  height: '9px',
                  background: color,
                  transformOrigin: 'bottom',
                  animation: `sp-eq 900ms ease-in-out ${d} infinite`,
              }}
          />
      ))}
    </span>
    );
});

const AvatarOrb = memo(function AvatarOrb({
                                              url,
                                              aura,
                                              glow,
                                          }: {
    url: string | null;
    aura: Aura;
    glow: string;
}) {
    const perf = usePerfMode();
    return (
        <div className="relative shrink-0 w-[84px] h-[84px] md:w-[100px] md:h-[100px]">
            <div
                className="absolute -inset-2 rounded-full pointer-events-none"
                style={{
                    background: `radial-gradient(circle, ${auraRgba(aura, 0.45)}, transparent 70%)`,
                    filter: perf.glow ? 'blur(10px)' : undefined,
                }}
            />
            <div
                className="relative w-full h-full rounded-full overflow-hidden"
                style={{border: `0.5px solid ${auraRgba(aura, 0.4)}`, boxShadow: `0 10px 34px ${glow}`}}
            >
                {url ? (
                    <img src={url} alt="" className="w-full h-full object-cover" decoding="async"/>
                ) : (
                    <div className="w-full h-full flex items-center justify-center bg-white/5">
                        <UserIcon size={30} className="text-white/25"/>
                    </div>
                )}
            </div>
        </div>
    );
});

/** "Wave" masthead — your taste as a living, star-lit portrait, with the
 *  vibe-search doorway carved in at the bottom. Darker frost than Library's. */
export const WaveMasthead = memo(function WaveMasthead({
                                                           user,
                                                           likedTracks,
                                                           sound,
                                                           selected,
                                                           onSelect,
                                                       }: {
    user: MastheadUser;
    likedTracks: Track[];
    sound: Soundprint;
    selected: string | null;
    onSelect: (genre: string | null) => void;
}) {
    const {t} = useTranslation();
    const perf = usePerfMode();
    const b = perf.blur(22);
    const avatar = art(user.avatar_url, 't300x300');

    return (
        <section
            className="relative overflow-hidden rounded-[2.25rem] p-6 md:p-8 transition-[box-shadow] duration-500"
            style={{
                border: '0.5px solid rgba(255,255,255,0.1)',
                boxShadow: `0 30px 80px rgba(0,0,0,0.5), 0 0 70px ${sound.accentGlow}`,
            }}
        >
            {/* frost — darker than the Library masthead */}
            <div
                className="absolute inset-0 rounded-[inherit] transition-[background] duration-500"
                style={{
                    contain: 'strict',
                    transform: 'translateZ(0)',
                    backdropFilter: b > 0 ? `blur(${b}px) saturate(150%)` : undefined,
                    WebkitBackdropFilter: b > 0 ? `blur(${b}px) saturate(150%)` : undefined,
                    background:
                        b > 0
                            ? `linear-gradient(145deg, ${auraRgba(sound.aura, 0.12)}, rgba(7,6,10,0.66))`
                            : 'rgba(10,9,13,0.94)',
                }}
            />
            <ArtworkMosaic tracks={likedTracks}/>
            {perf.atmosphere && (
                <StarField aura={sound.aura} seeds={HERO_STAR_SEEDS} intensity={0.95} glow={false}/>
            )}
            <div
                className="absolute inset-0 pointer-events-none rounded-[inherit] transition-[background] duration-500"
                style={{
                    background: `radial-gradient(120% 130% at 6% -12%, ${auraRgba(sound.aura, 0.22)}, transparent 58%)`,
                }}
            />

            <div className="relative z-10 flex flex-col gap-6" style={{isolation: 'isolate'}}>
                <div className="flex items-center gap-5">
                    <AvatarOrb url={avatar} aura={sound.aura} glow={sound.accentGlow}/>
                    <div className="min-w-0 flex-1">
                        <p className="flex items-center gap-1.5 text-[11px] uppercase tracking-[0.28em] text-white/40 font-bold mb-1.5">
                            <EqBars color={auraRgb(sound.aura)}/>
                            {t('home.yourWave')}
                        </p>
                        <h1
                            className="text-[26px] md:text-[34px] font-black tracking-tight leading-[1.05] break-words"
                            style={{
                                backgroundImage: sound.aura.nameGradient,
                                WebkitBackgroundClip: 'text',
                                backgroundClip: 'text',
                                color: 'transparent',
                            }}
                        >
                            {t(greetingKey(), {name: user.username})}
                        </h1>
                    </div>
                </div>

                {sound.hasData && (
                    <SoundprintBars spectrum={sound.spectrum} selected={selected} onSelect={onSelect}/>
                )}

                <VibePortal/>
            </div>
        </section>
    );
});
