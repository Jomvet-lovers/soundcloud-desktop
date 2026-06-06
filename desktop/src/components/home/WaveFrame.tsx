import {memo, type ReactNode} from 'react';
import {usePerfMode} from '../../lib/perf';
import {LIBRARY_KEYFRAMES} from '../library/keyframes';
import type {Soundprint} from '../library/useSoundprint';
import {Atmosphere} from '../search/Atmosphere';
import {USER_PAGE_KEYFRAMES} from '../user/keyframes';
import {PAGE_STAR_SEEDS, StarField} from '../user/StarField';

/** Shell for the "Wave" home — a deeper, star-lit room. The genre-aura
 *  atmosphere bleeds to the listener's taste, a star field drifts across the
 *  whole scroll, and a vignette pulls the page darker than the rest of the app.
 *  All motion gates on perf.atmosphere/idleAnim via the reused pieces. */
export const WaveFrame = memo(function WaveFrame({
                                                     sound,
                                                     children,
                                                 }: {
    sound: Soundprint;
    children: ReactNode;
}) {
    const perf = usePerfMode();
    return (
        <div className="relative min-h-full w-full">
            <style>{LIBRARY_KEYFRAMES + USER_PAGE_KEYFRAMES}</style>
            {/* deepen the room — darker than the rest of the app */}
            <div
                aria-hidden
                className="absolute inset-0 pointer-events-none"
                style={{
                    background:
                        'radial-gradient(125% 90% at 50% -8%, transparent 38%, rgba(0,0,0,0.55) 100%)',
                }}
            />
            {perf.atmosphere && <Atmosphere tint={sound.tint} energy={sound.energy}/>}
            {perf.atmosphere && <StarField aura={sound.aura} seeds={PAGE_STAR_SEEDS} intensity={0.7}/>}
            <div
                className="relative z-10 min-h-full max-w-[1320px] mx-auto px-4 md:px-8 pt-5 pb-6 space-y-8"
                style={{isolation: 'isolate'}}
            >
                {children}
            </div>
        </div>
    );
});
