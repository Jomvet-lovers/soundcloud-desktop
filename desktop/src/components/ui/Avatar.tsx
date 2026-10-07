import { art } from '../../lib/formatters';

interface AvatarProps {
  src?: string | null;
  alt?: string;
  size?: number;
  className?: string;
  /** Corner style — circle by default; `rounded` matches the sidebar's small
   *  artwork token (18px tile, 5px radius). */
  shape?: 'circle' | 'rounded';
}

export function Avatar({
  src,
  alt = '',
  size = 32,
  className = '',
  shape = 'circle',
}: AvatarProps) {
  const sizeStyle = { width: size, height: size, minWidth: size };
  const radius = shape === 'rounded' ? 'rounded-[5px]' : 'rounded-full';

  if (!src || src.includes('default_avatar')) {
    return (
      <div
        className={`${radius} bg-bg-glass-active flex items-center justify-center text-text-tertiary ${className}`}
        style={sizeStyle}
      >
        <svg width={size * 0.5} height={size * 0.5} viewBox="0 0 16 16" fill="currentColor">
          <circle cx="8" cy="5.5" r="3" />
          <path d="M2 14.5c0-3.3 2.7-6 6-6s6 2.7 6 6" />
        </svg>
      </div>
    );
  }

  return (
    <img
      src={art(src, 't200x200') ?? undefined}
      alt={alt}
      loading="lazy"
      decoding="async"
      className={`${radius} object-cover ${className}`}
      style={sizeStyle}
    />
  );
}
