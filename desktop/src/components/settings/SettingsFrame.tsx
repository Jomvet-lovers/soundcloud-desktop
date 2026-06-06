import {memo, type ReactNode} from 'react';
import type {Aura} from '../../lib/aura';
import {usePerfMode} from '../../lib/perf';
import {LIBRARY_KEYFRAMES} from '../library/keyframes';
import {Atmosphere} from '../search/Atmosphere';
import {USER_PAGE_KEYFRAMES} from '../user/keyframes';
import {PAGE_STAR_SEEDS, StarField} from '../user/StarField';

/** Atmospheric shell for Settings — the same star-lit, aura-tinted room the rest
 *  of the app wears, so the page stops reading as a flat black sheet. Tinted by
 *  the viewer's accent; all motion gates on perf. */
export const SettingsFrame = memo(function SettingsFrame({
                                                             aura,
                                                             children,
                                                         }: {
    aura: Aura;
    children: ReactNode;
}) {
    const perf = usePerfMode();
    return (
        <div className="relative min-h-full w-full">
            <style>{LIBRARY_KEYFRAMES + USER_PAGE_KEYFRAMES}</style>
            {perf.atmosphere && <Atmosphere tint={[...aura.orbs]} energy={0.32}/>}
            {/* star field — viewport-pinned (fixed) so it never cuts off at the
          content's bottom edge, exactly like Atmosphere */}
            {perf.atmosphere && (
                <div
                    aria-hidden
                    className="fixed inset-0 pointer-events-none overflow-hidden"
                    style={{contain: 'strict'}}
                >
                    <StarField aura={aura} seeds={PAGE_STAR_SEEDS} intensity={0.6} glow={false}/>
                </div>
            )}
            <div className="relative z-10">{children}</div>
        </div>
    );
});
