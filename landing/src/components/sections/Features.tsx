import { BanIcon, Cpu, Globe, Languages, Music, ShieldOff, SmartphoneNfc, Zap } from 'lucide-react';
import { features } from '../../constants';
import { Reveal } from '../ui/Reveal';

const icons = [ShieldOff, BanIcon, Globe, Music, Zap, Cpu, SmartphoneNfc, Languages];

export function Features() {
  return (
    <section className="section-gap" id="features">
      <div className="max-w-6xl mx-auto">
        <Reveal className="text-center mb-16">
          <p className="text-xs uppercase tracking-[0.2em] text-[#ff5500] mb-4 font-medium">
            Возможности
          </p>
          <h2
            className="text-3xl sm:text-5xl font-bold mb-4 tracking-tight"
            style={{ fontFamily: "'Satoshi', sans-serif" }}
          >
            Почему выбирают нас
          </h2>
          <p className="text-white/40 text-lg max-w-md mx-auto">
            Всё, чего не хватает в веб-версии SoundCloud
          </p>
        </Reveal>

        <div className="grid sm:grid-cols-2 lg:grid-cols-4 gap-4 reveal-stagger">
          {features.map((f, i) => {
            const Icon = icons[i];
            return (
              <Reveal key={f.title}>
                <div className="glass feature-card p-6 h-full flex flex-col">
                  <div
                    className="feature-icon text-[#ff5500] mb-5 w-10 h-10 rounded-xl flex items-center justify-center"
                    style={{ background: 'rgba(255, 85, 0, 0.08)' }}
                  >
                    <Icon size={24} />
                  </div>
                  <h3 className="text-[15px] font-semibold mb-2 text-white/90">{f.title}</h3>
                  <p className="text-white/40 text-[13px] leading-relaxed">{f.desc}</p>
                </div>
              </Reveal>
            );
          })}
        </div>
      </div>
    </section>
  );
}
