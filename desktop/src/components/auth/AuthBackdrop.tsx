import {memo} from 'react';
import {usePerfMode} from '../../lib/perf';
import {useViewerAura} from '../../lib/useViewerAura';
import {LIBRARY_KEYFRAMES} from '../library/keyframes';
import {Atmosphere} from '../search/Atmosphere';
import {USER_PAGE_KEYFRAMES} from '../user/keyframes';
import {PAGE_STAR_SEEDS, StarField} from '../user/StarField';
import {AUTH_KEYFRAMES} from './auth-keyframes';

/** Immersive, viewport-pinned login backdrop: aura orbs + a star field. */
export const AuthBackdrop = memo(function AuthBackdrop() {
    const perf = usePerfMode();
    const aura = useViewerAura();
    return (
        <>
            <style>{LIBRARY_KEYFRAMES + USER_PAGE_KEYFRAMES + AUTH_KEYFRAMES}</style>
            {perf.atmosphere && <Atmosphere tint={[...aura.orbs]} energy={0.5}/>}
            {perf.atmosphere && (
                <div
                    aria-hidden
                    className="fixed inset-0 pointer-events-none overflow-hidden"
                    style={{contain: 'strict'}}
                >
                    <StarField aura={aura} seeds={PAGE_STAR_SEEDS} intensity={0.85} glow={false}/>
                </div>
            )}
        </>
    );
});
