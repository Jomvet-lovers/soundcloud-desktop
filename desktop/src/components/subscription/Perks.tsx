import { memo } from 'react';
import { useTranslation } from 'react-i18next';

const PERKS = [
  { n: 'P-01', t: 'goPlus' },
  { n: 'P-02', t: 'server' },
  { n: 'P-03', t: 'hq' },
  { n: 'P-04', t: 'whitelist' },
  { n: 'P-05', t: 'soundwave' },
  { n: 'P-06', t: 'support' },
] as const;

export const Perks = memo(function Perks() {
  const { t } = useTranslation();
  return (
    <div
      className="grid grid-cols-1 gap-px overflow-hidden rounded-2xl border border-white/[0.06] sm:grid-cols-2 lg:grid-cols-3"
      style={{ background: 'rgba(255,255,255,0.06)' }}
    >
      {PERKS.map((p) => (
        <div
          key={p.n}
          className="flex flex-col gap-2 px-5 pb-[22px] pt-5 transition-colors duration-300 hover:bg-white/[0.03]"
          style={{ background: 'rgb(13,13,16)' }}
        >
          <span className="font-mono text-[11px] tracking-[0.1em] text-accent">{p.n}</span>
          <h4 className="text-[15px] font-semibold text-white/90">
            {t(`starpass.perk.${p.t}.title`)}
          </h4>
          <p className="text-[13px] leading-[1.45] text-white/55">
            {t(`starpass.perk.${p.t}.body`)}
          </p>
        </div>
      ))}
    </div>
  );
});
