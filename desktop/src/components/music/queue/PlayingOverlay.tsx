import React from 'react';
import {pauseTextWhite12} from '../../../lib/icons';

/** Artwork overlay shown on the playing track: animated EQ bars while playing,
 *  a pause glyph when paused. Shared by the now-playing card and the queue row. */
export const PlayingOverlay = React.memo(({isPlaying}: { isPlaying: boolean }) => (
    <div className="absolute inset-0 bg-black/45 flex items-center justify-center">
        {isPlaying ? (
            <div className="flex items-end gap-[2px]">
                <span className="w-[2px] h-3 bg-accent rounded-full animate-pulse"/>
                <span className="w-[2px] h-2 bg-accent rounded-full animate-pulse [animation-delay:150ms]"/>
                <span className="w-[2px] h-3.5 bg-accent rounded-full animate-pulse [animation-delay:300ms]"/>
            </div>
        ) : (
            pauseTextWhite12
        )}
    </div>
));
