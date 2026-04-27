import * as Dialog from '@radix-ui/react-dialog';
import React, { useCallback, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ExternalLink, Star, X } from '../../lib/icons';
import { useSubscription } from '../../lib/subscription';
import { useAuthStore } from '../../stores/auth';

/* ── Animated star particles (CSS-only, GPU-composited) ─────────── */

const PARTICLES = Array.from({ length: 12 }, (_, i) => i);

const StarParticles = React.memo(() => (
  <div
    className="absolute inset-0 overflow-hidden pointer-events-none"
    style={{ contain: 'strict', transform: 'translateZ(0)' }}
  >
    {PARTICLES.map((i) => (
      <div
        key={i}
        className="absolute w-[3px] h-[3px] rounded-full"
        style={{
          background: `hsl(${260 + ((i * 20) % 60)}, 80%, ${70 + ((i * 5) % 30)}%)`,
          left: `${10 + ((i * 37) % 80)}%`,
          top: `${10 + ((i * 53) % 80)}%`,
          opacity: 0.4 + (i % 3) * 0.2,
          animation: `star-float ${3 + (i % 3)}s ease-in-out ${(i * 0.4) % 3}s infinite alternate`,
        }}
      />
    ))}
  </div>
));

/* ── Star Hero Background (premium aura behind UserPage hero) ───── */

const HERO_STARS = Array.from({ length: 28 }, (_, i) => ({
  i,
  size: 6 + ((i * 7) % 14),
  left: (i * 37) % 100,
  top: (i * 53) % 100,
  rotate: (i * 41) % 360,
  hue: 250 + ((i * 13) % 70),
  delay: (i * 0.27) % 5,
  duration: 4 + (i % 5),
  opacity: 0.25 + (i % 4) * 0.15,
}));

const HERO_DOTS = Array.from({ length: 36 }, (_, i) => ({
  i,
  size: 2 + (i % 3),
  left: (i * 71) % 100,
  top: (i * 29) % 100,
  hue: 260 + ((i * 17) % 60),
  delay: (i * 0.31) % 4,
  duration: 3 + (i % 4),
  opacity: 0.3 + (i % 3) * 0.2,
}));

export const StarHeroBackground = React.memo(() => (
  <div
    className="absolute inset-0 pointer-events-none overflow-hidden"
    style={{ contain: 'strict', transform: 'translateZ(0)' }}
  >
    {/* Purple aura layers */}
    <div
      className="absolute inset-0"
      style={{
        background:
          'radial-gradient(ellipse 80% 60% at 50% 0%, rgba(168,85,247,0.28), transparent 70%), radial-gradient(ellipse 60% 50% at 20% 100%, rgba(139,92,246,0.18), transparent 70%), radial-gradient(ellipse 50% 50% at 90% 80%, rgba(192,132,252,0.16), transparent 70%)',
      }}
    />
    {/* Diagonal sparkle sheen */}
    <div
      className="absolute inset-0 opacity-40"
      style={{
        background:
          'linear-gradient(120deg, transparent 0%, transparent 40%, rgba(192,132,252,0.08) 50%, transparent 60%, transparent 100%)',
      }}
    />
    {/* Tiny dots */}
    {HERO_DOTS.map((d) => (
      <div
        key={`d-${d.i}`}
        className="absolute rounded-full"
        style={{
          width: `${d.size}px`,
          height: `${d.size}px`,
          left: `${d.left}%`,
          top: `${d.top}%`,
          background: `hsl(${d.hue}, 80%, 75%)`,
          opacity: d.opacity,
          boxShadow: `0 0 ${d.size * 2}px hsl(${d.hue}, 90%, 70%)`,
          animation: `star-float ${d.duration}s ease-in-out ${d.delay}s infinite alternate`,
        }}
      />
    ))}
    {/* Star icons scattered */}
    {HERO_STARS.map((s) => (
      <div
        key={`s-${s.i}`}
        className="absolute"
        style={{
          left: `${s.left}%`,
          top: `${s.top}%`,
          color: `hsl(${s.hue}, 85%, 75%)`,
          opacity: s.opacity,
          transform: `rotate(${s.rotate}deg)`,
          filter: `drop-shadow(0 0 ${s.size}px hsl(${s.hue}, 90%, 70%))`,
          animation: `star-float ${s.duration}s ease-in-out ${s.delay}s infinite alternate`,
        }}
      >
        <Star size={s.size} fill="currentColor" />
      </div>
    ))}
  </div>
));

/* ── Star Badge (next to username / plan badge) ─────────────────── */

interface StarBadgeProps {
  size?: 'sm' | 'lg';
}

export const StarBadge = React.memo(({ size = 'sm' }: StarBadgeProps) => {
  const isLg = size === 'lg';
  return (
    <span
      className={`inline-flex items-center shrink-0 rounded-full uppercase text-white/95 ${
        isLg
          ? 'gap-1.5 px-3 py-1 text-[10px] font-extrabold tracking-widest'
          : 'gap-[3px] px-[6px] py-[1px] text-[9px] font-bold tracking-wider'
      }`}
      style={{
        background:
          'linear-gradient(135deg, rgba(139,92,246,0.45), rgba(168,85,247,0.32), rgba(192,132,252,0.25))',
        backdropFilter: 'blur(12px)',
        WebkitBackdropFilter: 'blur(12px)',
        boxShadow: isLg
          ? 'inset 0 0.5px 0 rgba(255,255,255,0.2), 0 0 20px rgba(139,92,246,0.4)'
          : 'inset 0 0.5px 0 rgba(255,255,255,0.15), 0 0 8px rgba(139,92,246,0.25)',
        border: '0.5px solid rgba(168,85,247,0.35)',
      }}
    >
      <Star
        size={isLg ? 12 : 10}
        fill="currentColor"
        className="text-amber-400"
        style={isLg ? { filter: 'drop-shadow(0 0 4px rgba(168,85,247,0.6))' } : undefined}
      />
      Star
    </span>
  );
});

/* ── Star Card (sidebar, above collapse button) ────────────────── */

interface StarCardProps {
  collapsed: boolean;
  isPremium: boolean;
  onOpenModal: () => void;
}

export const StarCard = React.memo(({ collapsed, isPremium, onOpenModal }: StarCardProps) => {
  const { t } = useTranslation();

  if (collapsed) {
    return (
      <button
        type="button"
        onClick={onOpenModal}
        title={t('star.title')}
        className="relative flex items-center justify-center w-full py-2.5 rounded-xl cursor-pointer transition-all duration-200 hover:scale-105 group"
        style={{
          background: 'linear-gradient(135deg, rgba(139,92,246,0.2), rgba(168,85,247,0.12))',
          border: '0.5px solid rgba(168,85,247,0.2)',
        }}
      >
        <span
          className="text-amber-400"
          style={{ filter: 'drop-shadow(0 0 4px rgba(168,85,247,0.6))' }}
        >
          <Star size={16} fill="currentColor" />
        </span>
        <StarParticles />
      </button>
    );
  }

  return (
    <button
      type="button"
      onClick={onOpenModal}
      className="relative flex items-center gap-2.5 w-full px-3 py-2.5 rounded-xl cursor-pointer transition-all duration-200 hover:scale-[1.01] overflow-hidden group"
      style={{
        background:
          'linear-gradient(135deg, rgba(139,92,246,0.18), rgba(168,85,247,0.1), rgba(192,132,252,0.08))',
        backdropFilter: 'blur(16px)',
        WebkitBackdropFilter: 'blur(16px)',
        border: '0.5px solid rgba(168,85,247,0.2)',
        boxShadow: '0 2px 12px rgba(139,92,246,0.12), inset 0 0.5px 0 rgba(255,255,255,0.08)',
      }}
    >
      <StarParticles />
      <div className="relative flex items-center gap-2.5" style={{ isolation: 'isolate' }}>
        <span
          className="text-amber-400 shrink-0"
          style={{ filter: 'drop-shadow(0 0 6px rgba(168,85,247,0.5))' }}
        >
          <Star size={16} fill="currentColor" />
        </span>
        <div className="flex flex-col items-start min-w-0">
          <span className="text-[11px] font-semibold text-white/90 tracking-wide">
            {t('star.title')}
          </span>
          <span className="text-[9px] text-purple-300/60 font-medium">
            {isPremium ? t('star.active') : t('star.getIt')}
          </span>
        </div>
      </div>
    </button>
  );
});

/* ── Star Modal ─────────────────────────────────────────────────── */

const PERKS = [
  'star.perkGoPlus',
  'star.perkServer',
  'star.perkHQ',
  'star.bypassWhitelist',
  'star.perkSupport',
] as const;

const STEPS = [
  { key: 'star.step1', link: 'https://boosty.to/lolinamide' },
  { key: 'star.step2' },
  { key: 'star.step3', link: 'https://discord.gg/xQcGBP8fGG' },
  { key: 'star.step4' },
  { key: 'star.step5' },
] as const;

const MODAL_PARTICLES = Array.from({ length: 20 }, (_, i) => i);

export const StarModal = React.memo(
  ({ open, onOpenChange }: { open: boolean; onOpenChange: (v: boolean) => void }) => {
    const { t } = useTranslation();

    return (
      <Dialog.Root open={open} onOpenChange={onOpenChange}>
        <Dialog.Portal>
          <Dialog.Overlay className="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm animate-in fade-in duration-200" />
          <Dialog.Content
            className="fixed left-1/2 top-1/2 z-50 w-[420px] max-w-[90vw] max-h-[85vh] -translate-x-1/2 -translate-y-1/2 rounded-2xl overflow-hidden outline-none animate-in fade-in zoom-in-95 duration-200 flex flex-col"
            style={{
              background:
                'linear-gradient(165deg, rgba(30,15,50,0.95), rgba(20,10,40,0.97), rgba(15,8,30,0.98))',
              border: '0.5px solid rgba(168,85,247,0.25)',
              boxShadow:
                '0 25px 60px rgba(0,0,0,0.5), 0 0 40px rgba(139,92,246,0.15), inset 0 1px 0 rgba(255,255,255,0.05)',
            }}
          >
            {/* Animated background particles */}
            <div
              className="absolute inset-0 overflow-hidden pointer-events-none"
              style={{ contain: 'strict', transform: 'translateZ(0)' }}
            >
              {MODAL_PARTICLES.map((i) => (
                <div
                  key={i}
                  className="absolute rounded-full"
                  style={{
                    width: `${2 + (i % 3)}px`,
                    height: `${2 + (i % 3)}px`,
                    background: `hsl(${250 + ((i * 15) % 70)}, 75%, ${65 + ((i * 7) % 30)}%)`,
                    left: `${5 + ((i * 31) % 90)}%`,
                    top: `${5 + ((i * 47) % 90)}%`,
                    opacity: 0.3 + (i % 4) * 0.15,
                    animation: `star-float ${4 + (i % 4)}s ease-in-out ${(i * 0.3) % 4}s infinite alternate`,
                  }}
                />
              ))}
            </div>

            {/* Gradient glow top */}
            <div
              className="absolute top-0 left-1/2 -translate-x-1/2 w-3/4 h-32 pointer-events-none"
              style={{
                background: 'radial-gradient(ellipse, rgba(139,92,246,0.2) 0%, transparent 70%)',
                transform: 'translateZ(0)',
              }}
            />

            <div
              className="relative overflow-y-auto p-6 star-scroll"
              style={{ isolation: 'isolate' }}
            >
              {/* Close */}
              <Dialog.Close className="absolute top-4 right-4 p-1 rounded-lg text-white/30 hover:text-white/70 hover:bg-white/[0.06] transition-colors cursor-pointer">
                <X size={16} />
              </Dialog.Close>

              {/* Header */}
              <div className="flex flex-col items-center text-center mb-6">
                <span
                  className="text-amber-400 mb-3"
                  style={{ filter: 'drop-shadow(0 0 12px rgba(168,85,247,0.6))' }}
                >
                  <Star size={36} fill="currentColor" />
                </span>
                <Dialog.Title className="flex items-center gap-2 text-xl font-bold text-white/95 tracking-tight">
                  <Star size={20} fill="currentColor" className="text-amber-400" />
                  {t('star.modalTitle')}
                </Dialog.Title>
                <p className="text-[12px] text-purple-300/50 mt-1 font-medium">
                  {t('star.modalSub')}
                </p>
              </div>

              {/* Perks */}
              <div className="space-y-2 mb-6">
                {PERKS.map((perk) => (
                  <div
                    key={perk}
                    className="flex items-start gap-3 px-3.5 py-2.5 rounded-xl"
                    style={{
                      background:
                        'linear-gradient(135deg, rgba(139,92,246,0.1), rgba(168,85,247,0.05))',
                      border: '0.5px solid rgba(168,85,247,0.12)',
                    }}
                  >
                    <span className="text-purple-400/80 text-[13px] mt-px shrink-0">✦</span>
                    <span className="text-[12.5px] text-white/75 leading-relaxed">{t(perk)}</span>
                  </div>
                ))}
              </div>

              {/* Divider */}
              <div
                className="h-px mb-5"
                style={{
                  background:
                    'linear-gradient(90deg, transparent, rgba(168,85,247,0.2), transparent)',
                }}
              />

              {/* How to get */}
              <div className="mb-2">
                <h3 className="text-[12px] font-semibold text-white/60 uppercase tracking-wider mb-3">
                  {t('star.howTo')}
                </h3>
                <div className="space-y-2.5">
                  {STEPS.map((step, i) => (
                    <div key={step.key} className="flex items-start gap-3">
                      <span
                        className="shrink-0 w-5 h-5 rounded-full flex items-center justify-center text-[10px] font-bold text-purple-300/80 mt-0.5"
                        style={{
                          background:
                            'linear-gradient(135deg, rgba(139,92,246,0.2), rgba(168,85,247,0.1))',
                          border: '0.5px solid rgba(168,85,247,0.2)',
                        }}
                      >
                        {i + 1}
                      </span>
                      <div className="flex-1 min-w-0">
                        <span className="text-[12px] text-white/65 leading-relaxed">
                          {t(step.key)}
                        </span>
                        {'link' in step && step.link && (
                          <a
                            href={step.link}
                            target="_blank"
                            rel="noopener noreferrer"
                            className="inline-flex items-center gap-1 mt-1 px-2.5 py-1 rounded-lg text-[11px] font-medium text-purple-300/80 hover:text-purple-200 transition-colors cursor-pointer"
                            style={{
                              background:
                                'linear-gradient(135deg, rgba(139,92,246,0.15), rgba(168,85,247,0.08))',
                              border: '0.5px solid rgba(168,85,247,0.2)',
                            }}
                          >
                            {t(i === 0 ? 'star.goBoosty' : 'star.goDiscord')}
                            <ExternalLink size={10} />
                          </a>
                        )}
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            </div>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>
    );
  },
);

/* ── Sidebar integration hook ───────────────────────────────────── */

export function useStarSubscription() {
  const isAuthenticated = useAuthStore((s) => s.isAuthenticated);
  const { data: isPremium } = useSubscription(isAuthenticated);
  const [modalOpen, setModalOpen] = useState(false);
  const openModal = useCallback(() => setModalOpen(true), []);

  return { isPremium: !!isPremium, modalOpen, setModalOpen, openModal };
}
