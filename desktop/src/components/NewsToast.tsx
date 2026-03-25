import * as Dialog from '@radix-ui/react-dialog';
import React, { useCallback, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { type NewsItem, NEWS } from '../lib/news';
import { X } from '../lib/icons';
import { useNewsStore } from '../stores/news';

// ─── Toast Card (bottom-left) ──────────────────────────────

const accentBorder: Record<string, string> = {
  violet: 'border-violet-400/20 hover:border-violet-400/30',
  amber: 'border-amber-400/20 hover:border-amber-400/30',
  sky: 'border-sky-400/20 hover:border-sky-400/30',
  emerald: 'border-emerald-400/20 hover:border-emerald-400/30',
};

const accentGlow: Record<string, string> = {
  violet: 'shadow-[0_0_30px_rgba(139,92,246,0.08)]',
  amber: 'shadow-[0_0_30px_rgba(251,191,36,0.08)]',
  sky: 'shadow-[0_0_30px_rgba(56,189,248,0.08)]',
  emerald: 'shadow-[0_0_30px_rgba(52,211,153,0.08)]',
};

const accentDot: Record<string, string> = {
  violet: 'bg-violet-400',
  amber: 'bg-amber-400',
  sky: 'bg-sky-400',
  emerald: 'bg-emerald-400',
};

const accentModalBorder: Record<string, string> = {
  violet: 'border-violet-400/14',
  amber: 'border-amber-400/14',
  sky: 'border-sky-400/14',
  emerald: 'border-emerald-400/14',
};

const SingleNewsToast = React.memo(function SingleNewsToast({
  item,
  index,
}: {
  item: NewsItem;
  index: number;
}) {
  const { t } = useTranslation();
  const [modalOpen, setModalOpen] = useState(false);
  const dismissOnce = useNewsStore((s) => s.dismissOnce);
  const dismissForever = useNewsStore((s) => s.dismissForever);

  const accent = item.accent ?? 'violet';
  const border = accentBorder[accent] ?? accentBorder.violet;
  const glow = accentGlow[accent] ?? accentGlow.violet;
  const dot = accentDot[accent] ?? accentDot.violet;
  const mBorder = accentModalBorder[accent] ?? accentModalBorder.violet;

  const handleDismissOnce = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation();
      dismissOnce(item.id);
    },
    [dismissOnce, item.id],
  );

  const handleDismissForever = useCallback(() => {
    dismissForever(item.id);
    setModalOpen(false);
  }, [dismissForever, item.id]);

  return (
    <Dialog.Root open={modalOpen} onOpenChange={setModalOpen}>
      {/* Toast */}
      <div
        className={`animate-in slide-in-from-left-4 fade-in duration-500 fill-mode-both`}
        style={{ animationDelay: `${index * 120}ms` }}
      >
        <Dialog.Trigger asChild>
          <button
            type="button"
            className={`group relative flex w-[340px] cursor-pointer items-start gap-3.5 rounded-2xl border bg-[#1a1a1e]/90 px-4 py-3.5 text-left backdrop-blur-xl transition-all duration-300 ease-[var(--ease-apple)] ${border} ${glow} hover:bg-[#1e1e24]/95 hover:scale-[1.01]`}
          >
            {/* Accent dot */}
            <div className={`mt-1.5 size-2 shrink-0 rounded-full ${dot} animate-pulse`} />

            <div className="min-w-0 flex-1">
              <div className="text-[13px] font-semibold text-white/90 leading-tight">
                {t(item.titleKey)}
              </div>
              <div className="mt-1 text-[12px] leading-relaxed text-white/45 line-clamp-2">
                {t(item.descriptionKey)}
              </div>
            </div>

            {item.image && (
              <img
                src={item.image}
                alt=""
                className="size-11 shrink-0 rounded-xl object-cover ring-1 ring-white/[0.06]"
                decoding="async"
              />
            )}

            {/* Close (dismiss once) */}
            <button
              type="button"
              onClick={handleDismissOnce}
              className="absolute right-2 top-2 flex size-6 items-center justify-center rounded-lg bg-white/0 text-white/0 transition-all group-hover:bg-white/[0.06] group-hover:text-white/40 hover:!bg-white/[0.1] hover:!text-white/60"
            >
              <X size={12} />
            </button>
          </button>
        </Dialog.Trigger>
      </div>

      {/* Modal */}
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-[9999] bg-black/55 backdrop-blur-sm animate-in fade-in duration-200" />
        <Dialog.Content className={`fixed left-1/2 top-1/2 z-[10000] w-full max-w-md -translate-x-1/2 -translate-y-1/2 rounded-2xl border bg-[#1a1a1e]/95 backdrop-blur-2xl shadow-[0_8px_64px_rgba(0,0,0,0.6)] overflow-hidden animate-in fade-in zoom-in-95 duration-200 ${mBorder}`}>
          {/* Header */}
          <div className="flex items-center justify-between px-5 pt-5 pb-3">
            <div className="flex items-center gap-2.5">
              <div className={`size-2.5 rounded-full ${dot}`} />
              <Dialog.Title className="text-[15px] font-semibold text-white/92">
                {t(item.titleKey)}
              </Dialog.Title>
            </div>
            <Dialog.Close className="flex size-7 cursor-pointer items-center justify-center rounded-lg bg-white/[0.05] text-white/40 transition-colors hover:bg-white/[0.1] hover:text-white/60">
              <X size={14} />
            </Dialog.Close>
          </div>

          {/* Image */}
          {item.image && (
            <div className="px-5 pb-3">
              <img
                src={item.image}
                alt=""
                className="w-full rounded-xl object-cover ring-1 ring-white/[0.06]"
                decoding="async"
              />
            </div>
          )}

          {/* Body */}
          <div className="px-5 pb-4">
            <p className="text-[13px] leading-relaxed text-white/60 whitespace-pre-line">
              {t(item.bodyKey)}
            </p>
          </div>

          {/* Actions */}
          <div className="flex gap-2 px-5 pb-5">
            <Dialog.Close className="flex-1 cursor-pointer rounded-xl bg-white/[0.05] py-2.5 text-[13px] font-medium text-white/50 transition-colors hover:bg-white/[0.08]">
              {t('news.dismissOnce')}
            </Dialog.Close>
            <button
              type="button"
              onClick={handleDismissForever}
              className="flex-1 cursor-pointer rounded-xl bg-white/[0.08] py-2.5 text-[13px] font-medium text-white/70 transition-colors hover:bg-white/[0.12]"
            >
              {t('news.dismissForever')}
            </button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
});

// ─── Container ──────────────────────────────────────────────

export const NewsToast = React.memo(function NewsToast() {
  const permanentlyDismissed = useNewsStore((s) => s.permanentlyDismissed);
  const sessionDismissed = useNewsStore((s) => s.sessionDismissed);

  const visible = useMemo(
    () =>
      NEWS.filter(
        (item) =>
          !permanentlyDismissed.includes(item.id) && !sessionDismissed.includes(item.id),
      ),
    [permanentlyDismissed, sessionDismissed],
  );

  if (visible.length === 0) return null;

  return (
    <div className="fixed bottom-20 left-4 z-[999] flex flex-col gap-2.5">
      {visible.map((item, i) => (
        <SingleNewsToast key={item.id} item={item} index={i} />
      ))}
    </div>
  );
});
