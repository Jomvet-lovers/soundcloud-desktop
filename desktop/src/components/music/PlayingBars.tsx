import React from 'react';

/** Spotify-style animated equalizer bars shown in place of the row number
 *  while a track is playing (paused bars freeze via data-playing). */
export const PlayingBars = React.memo(function PlayingBars({
  playing,
  className,
}: {
  playing: boolean;
  className?: string;
}) {
  return (
    <span
      className={`pb-bars ${className ?? ''}`}
      data-playing={playing ? 'true' : 'false'}
      aria-hidden="true"
    >
      <i />
      <i />
      <i />
    </span>
  );
});
