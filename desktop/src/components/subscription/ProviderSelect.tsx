import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { usePerfMode } from '../../lib/perf';
import { ProviderGlyph } from './ProviderGlyph';
import type { ActivationOption } from './providers';

interface ProviderSelectProps {
  options: ActivationOption[];
  selectedKind: ActivationOption['kind'] | null;
  onSelect: (opt: ActivationOption) => void;
}

export const ProviderSelect = memo(function ProviderSelect({
  options,
  selectedKind,
  onSelect,
}: ProviderSelectProps) {
  const { t } = useTranslation();
  const perf = usePerfMode();
  const blur = perf.blur(40);

  return (
    <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-5">
      {options.map((opt) => {
        const selected = opt.kind === selectedKind;
        return (
          <button
            key={opt.kind}
            type="button"
            aria-pressed={selected}
            onClick={() => onSelect(opt)}
            className="flex cursor-pointer flex-col items-start gap-3 rounded-2xl border px-4 py-5 text-left transition-all duration-300 ease-[var(--ease-apple)] hover:-translate-y-[2px]"
            style={{
              borderColor: selected
                ? 'color-mix(in srgb, var(--color-accent) 60%, transparent)'
                : 'rgba(255,255,255,0.06)',
              background: blur ? 'rgba(255,255,255,0.03)' : 'rgba(22,22,26,0.85)',
              backdropFilter: blur ? `blur(${blur}px)` : undefined,
              WebkitBackdropFilter: blur ? `blur(${blur}px)` : undefined,
              boxShadow: selected
                ? '0 0 0 1px color-mix(in srgb, var(--color-accent) 40%, transparent), 0 18px 40px -20px color-mix(in srgb, var(--color-accent) 70%, transparent)'
                : undefined,
            }}
          >
            <span
              className="grid size-[42px] place-items-center rounded-[11px] border border-white/[0.06]"
              style={{ background: 'rgba(255,255,255,0.04)' }}
            >
              <ProviderGlyph kind={opt.kind} />
            </span>
            <div>
              <h4 className="text-[14px] font-semibold text-white/90">
                {t(`starpass.method.${opt.i18n}.title`)}
              </h4>
              <p className="text-[12px] leading-[1.4] text-white/55">
                {t(`starpass.method.${opt.i18n}.body`)}
              </p>
            </div>
            <span className="mt-auto pt-1 font-mono text-[10px] tracking-[0.08em] text-white/35">
              {opt.tag}
            </span>
          </button>
        );
      })}
    </div>
  );
});
