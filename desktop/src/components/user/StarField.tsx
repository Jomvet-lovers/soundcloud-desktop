import React from 'react';
import { Star } from '../../lib/icons';
import type { Aura } from '../../lib/aura';

export type StarSeed = {
  i: number;
  size: number;
  left: number;
  top: number;
  rot: number;
  hueShift: number;
  delay: number;
  duration: number;
  min: number;
  max: number;
};

const makeSeeds = (count: number, salt = 0): StarSeed[] =>
  Array.from({ length: count }, (_, i) => {
    const k = i + salt;
    return {
      i,
      size: 6 + ((k * 7) % 14),
      left: (k * 37) % 100,
      top: (k * 53) % 100,
      rot: (k * 41) % 360,
      hueShift: ((k * 13) % 70) - 35,
      delay: (k * 0.27) % 5,
      duration: 4 + (k % 5),
      min: 0.18 + (k % 4) * 0.06,
      max: 0.55 + (k % 4) * 0.12,
    };
  });

const DOT_SEEDS = Array.from({ length: 44 }, (_, i) => ({
  i,
  size: 2 + (i % 3),
  left: (i * 71) % 100,
  top: (i * 29) % 100,
  delay: (i * 0.31) % 4,
  duration: 3 + (i % 4),
  min: 0.2 + (i % 3) * 0.1,
  max: 0.55 + (i % 3) * 0.12,
}));

export const HERO_STAR_SEEDS = makeSeeds(34, 1);
export const PAGE_STAR_SEEDS = makeSeeds(46, 7);

interface StarFieldProps {
  aura: Aura;
  seeds: StarSeed[];
  intensity?: number;
}

function StarFieldImpl({ aura, seeds, intensity = 1 }: StarFieldProps) {
  return (
    <div
      className="absolute inset-0 pointer-events-none overflow-hidden"
      style={{ contain: 'strict', transform: 'translateZ(0)' }}
    >
      {DOT_SEEDS.map((d) => {
        const color = aura.orbs[d.i % 3];
        return (
          <div
            key={`d-${d.i}`}
            className="absolute rounded-full"
            style={
              {
                width: `${d.size}px`,
                height: `${d.size}px`,
                left: `${d.left}%`,
                top: `${d.top}%`,
                background: color,
                boxShadow: `0 0 ${d.size * 2}px ${color}`,
                ['--min' as keyof React.CSSProperties]: d.min * intensity,
                ['--max' as keyof React.CSSProperties]: d.max * intensity,
                animation: `star-twinkle ${d.duration}s ease-in-out ${d.delay}s infinite`,
              } as React.CSSProperties
            }
          />
        );
      })}
      {seeds.map((s) => {
        const color = aura.orbs[s.i % 3];
        return (
          <div
            key={`s-${s.i}`}
            className="absolute"
            style={
              {
                left: `${s.left}%`,
                top: `${s.top}%`,
                color,
                transform: `rotate(${s.rot}deg)`,
                filter: `drop-shadow(0 0 ${s.size}px ${color})`,
                ['--min' as keyof React.CSSProperties]: s.min * intensity,
                ['--max' as keyof React.CSSProperties]: s.max * intensity,
                animation: `star-twinkle ${s.duration}s ease-in-out ${s.delay}s infinite`,
              } as React.CSSProperties
            }
          >
            <Star size={s.size} fill="currentColor" />
          </div>
        );
      })}
    </div>
  );
}

export const StarField = React.memo(StarFieldImpl);
