import { Brain, Music2, Sparkles, Wand2 } from 'lucide-react';
import { Reveal } from '../ui/Reveal';

const FEATURES = [
  {
    step: '01',
    title: 'Анализ звука',
    desc: 'Разбираем каждый трек на атомы — темп, тональность, энергетика, инструменты',
  },
  {
    step: '02',
    title: 'Понимание вайба',
    desc: 'Учитываем не только бит, но и настроение, атмосферу, эмоциональный окрас',
  },
  {
    step: '03',
    title: 'Анализ лирики',
    desc: 'Читаем тексты, понимаем смысл — грустное, мотивирующее, романтичное',
  },
  {
    step: '04',
    title: 'Твоя волна',
    desc: 'Собираем персональную ленту, которая учится на твоих лайках и скипах',
  },
];

const icons = [Music2, Brain, Wand2, Sparkles];

export function Soundwave() {
  return (
    <section className="section-gap relative overflow-hidden" id="soundwave">
      {/* Background gradient orbs */}
      <div className="orb orb-glow-lg w-[600px] h-[600px] bg-[#ff5500] -top-[200px] -right-[200px]" />
      <div className="orb orb-glow w-[400px] h-[400px] bg-[#ff7700] bottom-[10%] left-[10%]" />

      <div className="max-w-6xl mx-auto relative z-10">
        {/* Header */}
        <Reveal className="text-center mb-16">
          <div
            className="inline-flex items-center gap-2 px-4 py-2 rounded-full mb-6"
            style={{
              background: 'linear-gradient(135deg, rgba(255,85,0,0.15), rgba(255,119,0,0.08))',
              border: '1px solid rgba(255,85,0,0.2)',
            }}
          >
            <Sparkles size={14} className="text-[#ff5500]" />
            <span className="text-xs uppercase tracking-[0.2em] text-[#ff5500] font-bold">
              Soundwave
            </span>
          </div>
          <h2
            className="text-4xl sm:text-6xl font-bold mb-6 tracking-tight"
            style={{ fontFamily: "'Satoshi', sans-serif" }}
          >
            <span className="gradient-text">Новая эра</span>
            <br />
            <span className="text-white/90">рекомендаций</span>
          </h2>
          <p className="text-white/40 text-xl max-w-2xl mx-auto leading-relaxed">
            Забудь про скучные алгоритмы. Мы понимаем музыку так же, как ты — через эмоции и вайб
          </p>
        </Reveal>

        {/* Features grid */}
        <div className="grid sm:grid-cols-2 gap-6">
            {FEATURES.map((f, i) => {
              const Icon = icons[i];
              return (
                <Reveal key={f.step}>
                  <div
                    className="glass p-8 h-full flex flex-col relative group"
                    style={{
                      background:
                        'linear-gradient(135deg, rgba(255,255,255,0.05), rgba(255,255,255,0.02))',
                    }}
                  >
                    {/* Step number */}
                    <div
                      className="absolute -top-3 -left-3 w-12 h-12 rounded-2xl flex items-center justify-center font-bold text-sm"
                      style={{
                        background: 'linear-gradient(135deg, #ff5500, #ff7700)',
                        boxShadow: '0 8px 24px rgba(255,85,0,0.3)',
                      }}
                    >
                      {f.step}
                    </div>

                    {/* Icon */}
                    <div
                      className="w-14 h-14 rounded-2xl flex items-center justify-center mb-6 mt-4 transition-transform duration-300 group-hover:scale-110"
                      style={{
                        background: 'rgba(255,85,0,0.1)',
                        border: '1px solid rgba(255,85,0,0.2)',
                      }}
                    >
                      <Icon size={24} className="text-[#ff5500]" />
                    </div>

                    {/* Content */}
                    <h3 className="text-xl font-bold mb-3 text-white/90">{f.title}</h3>
                    <p className="text-white/50 text-[15px] leading-relaxed">{f.desc}</p>
                  </div>
                </Reveal>
              );
            })}
        </div>

        {/* Bottom CTA */}
        <Reveal className="text-center mt-16">
          <div
            className="inline-block px-8 py-4 rounded-2xl"
            style={{
              background: 'linear-gradient(135deg, rgba(255,85,0,0.08), rgba(255,119,0,0.04))',
              border: '1px solid rgba(255,85,0,0.15)',
            }}
          >
            <p className="text-white/60 text-sm mb-2">
              Работает на основе твоих лайков и скипов
            </p>
            <p className="text-[#ff5500] font-semibold text-lg">
              Чем больше слушаешь — тем точнее рекомендации
            </p>
          </div>
        </Reveal>
      </div>
    </section>
  );
}
