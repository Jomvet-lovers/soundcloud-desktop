import { ArrowRight, Download, Star } from 'lucide-react';
import { DISCORD, RELEASES } from '../../constants';
import { Reveal } from '../ui/Reveal';

const PERKS = [
  'Доступ к GO+ трекам',
  'Выделенный сервер стриминга',
  'Больше HQ треков',
  'Поддержка проекта',
];

export function StarSubscription() {
  return (
    <section className="section-gap relative" id="star">
      <div className="orb orb-glow w-[500px] h-[500px] bg-purple-500 top-[20%] left-[50%] -translate-x-1/2" />

      <div className="max-w-4xl mx-auto relative z-10">
        <div className="relative overflow-hidden rounded-[32px]">
            {/* Star particles background */}
            <div
              className="absolute inset-0 overflow-hidden pointer-events-none"
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
              {/* Animated stars */}
              {Array.from({ length: 28 }, (_, i) => ({
                i,
                size: 6 + ((i * 7) % 14),
                left: (i * 37) % 100,
                top: (i * 53) % 100,
                rotate: (i * 41) % 360,
                hue: 250 + ((i * 13) % 70),
                delay: (i * 0.27) % 5,
                duration: 4 + (i % 5),
                opacity: 0.25 + (i % 4) * 0.15,
              })).map((s) => (
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
              {/* Tiny dots */}
              {Array.from({ length: 36 }, (_, i) => ({
                i,
                size: 2 + (i % 3),
                left: (i * 71) % 100,
                top: (i * 29) % 100,
                hue: 260 + ((i * 17) % 60),
                delay: (i * 0.31) % 4,
                duration: 3 + (i % 4),
                opacity: 0.3 + (i % 3) * 0.2,
              })).map((d) => (
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
            </div>

            {/* Content */}
            <div
              className="relative p-10 sm:p-16"
              style={{
                background:
                  'linear-gradient(165deg, rgba(30,15,50,0.85), rgba(20,10,40,0.87), rgba(15,8,30,0.88))',
                border: '0.5px solid rgba(168,85,247,0.25)',
                boxShadow:
                  '0 25px 60px rgba(0,0,0,0.3), 0 0 40px rgba(139,92,246,0.15), inset 0 1px 0 rgba(255,255,255,0.05)',
                isolation: 'isolate',
              }}
            >
              <Reveal>
                {/* Header */}
                <div className="flex flex-col items-center text-center mb-8">
                  <span
                    className="text-amber-400 mb-4"
                    style={{ filter: 'drop-shadow(0 0 12px rgba(168,85,247,0.6))' }}
                  >
                    <Star size={48} fill="currentColor" />
                  </span>
                  <h2
                    className="text-3xl sm:text-5xl font-bold mb-3 tracking-tight flex items-center gap-3"
                    style={{ fontFamily: "'Satoshi', sans-serif" }}
                  >
                    <Star size={28} fill="currentColor" className="text-amber-400" />
                    <span className="gradient-text">Подписка Star</span>
                  </h2>
                  <p className="text-purple-300/60 text-lg mb-2">
                    Поддержи проект — получи эксклюзивные бонусы
                  </p>
                  <p className="text-2xl font-bold text-amber-400">
                    300 ₽
                  </p>
                </div>

                {/* Perks */}
                <div className="grid sm:grid-cols-2 gap-3 mb-8 max-w-2xl mx-auto">
                  {PERKS.map((perk) => (
                    <div
                      key={perk}
                      className="flex items-start gap-3 px-4 py-3 rounded-xl"
                      style={{
                        background:
                          'linear-gradient(135deg, rgba(139,92,246,0.15), rgba(168,85,247,0.08))',
                        border: '0.5px solid rgba(168,85,247,0.2)',
                      }}
                    >
                      <span className="text-purple-400/80 text-base mt-0.5 shrink-0">✦</span>
                      <span className="text-[14px] text-white/80 leading-relaxed">{perk}</span>
                    </div>
                  ))}
                </div>

                {/* CTA */}
                <div className="flex flex-col sm:flex-row gap-4 justify-center items-center">
                  <a
                    href="https://boosty.to/lolinamide"
                    target="_blank"
                    rel="noopener noreferrer"
                    className="btn-primary text-lg"
                    style={{
                      background:
                        'linear-gradient(135deg, rgba(139,92,246,0.9), rgba(168,85,247,0.8))',
                      boxShadow: '0 0 30px rgba(139,92,246,0.4)',
                    }}
                  >
                    <Star size={19} fill="currentColor" className="text-amber-400" />
                    Купить за 300 ₽
                    <ArrowRight size={16} className="opacity-50" />
                  </a>
                  <a
                    href={RELEASES}
                    className="btn-secondary text-lg"
                    style={{
                      background: 'rgba(255,255,255,0.03)',
                      border: '0.5px solid rgba(168,85,247,0.2)',
                    }}
                  >
                    <Download size={19} strokeWidth={2.5} />
                    Или скачай бесплатно
                  </a>
                </div>

                {/* Note */}
                <p className="text-center text-white/30 text-xs mt-6 max-w-md mx-auto">
                  Количество бонусов будет увеличиваться! Подробности в{' '}
                  <a
                    href={DISCORD}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="text-purple-400/70 hover:text-purple-300 transition-colors underline"
                  >
                    Discord
                  </a>
                </p>
              </Reveal>
            </div>
          </div>
      </div>
    </section>
  );
}
