import { ArrowRight, Download } from 'lucide-react';
import { siApple, siDebian, siFlatpak, siLinux, siRedhat } from 'simple-icons';
import { platforms, RELEASES, siWindows } from '../../constants';
import { Reveal } from '../ui/Reveal';
import { Si } from '../ui/Si';

const platformIcons = [siWindows, siDebian, siRedhat, siLinux, siFlatpak, siApple];

export function Platforms() {
  return (
    <section className="section-gap relative" id="download">
      {/* Background accent */}
      <div className="orb orb-glow w-[500px] h-[500px] bg-[#ff5500] top-[20%] left-[50%] -translate-x-1/2" />

      <div className="max-w-5xl mx-auto relative z-10">
        <Reveal className="text-center mb-16">
          <p className="text-xs uppercase tracking-[0.2em] text-[#ff5500] mb-4 font-medium">
            Скачать
          </p>
          <h2
            className="text-3xl sm:text-5xl font-bold mb-4 tracking-tight"
            style={{ fontFamily: "'Satoshi', sans-serif" }}
          >
            Доступно везде
          </h2>
          <p className="text-white/40 text-lg">6 форматов · 3 платформы · 2 архитектуры</p>
        </Reveal>

        <div className="grid sm:grid-cols-2 lg:grid-cols-3 gap-4 mb-14 reveal-stagger">
          {platforms.map((p, i) => (
            <Reveal key={p.name}>
              <a
                href={RELEASES}
                className="glass platform-card p-5 flex items-center gap-4 no-underline text-inherit group"
              >
                <div className="text-white/50 group-hover:text-[#ff5500] transition-colors shrink-0">
                  <Si icon={platformIcons[i]} className="w-7 h-7" />
                </div>
                <div className="flex-1 min-w-0">
                  <div className="font-semibold text-[15px] text-white/85">{p.name}</div>
                  <div className="text-white/30 text-xs mt-0.5 font-mono">{p.formats}</div>
                </div>
                <div className="text-white/15 text-[11px] shrink-0">{p.note}</div>
              </a>
            </Reveal>
          ))}
        </div>

        <Reveal className="text-center">
          <a href={RELEASES} className="btn-primary text-lg">
            <Download size={19} strokeWidth={2.5} />
            Скачать последнюю версию
            <ArrowRight size={16} className="opacity-50" />
          </a>
        </Reveal>
      </div>
    </section>
  );
}
