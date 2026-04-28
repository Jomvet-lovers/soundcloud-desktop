import { ArrowUpRight, Calendar, Sparkles, Waves } from 'lucide-react';
import { Link } from 'react-router-dom';
import { Reveal } from '../ui/Reveal';

export function NewsSection() {
  return (
    <section className="section-gap relative overflow-hidden" id="news">
      <div className="orb orb-glow-lg w-[600px] h-[600px] bg-[#a855f7] -top-[200px] -left-[100px]" />
      <div className="orb orb-glow w-[400px] h-[400px] bg-[#37c4ff] bottom-[10%] right-[10%]" />

      <div className="max-w-6xl mx-auto relative z-10">
        <Reveal className="text-center mb-12">
          <div
            className="inline-flex items-center gap-2 px-4 py-2 rounded-full mb-6"
            style={{
              background: 'linear-gradient(135deg, rgba(168,85,247,0.15), rgba(55,196,255,0.08))',
              border: '1px solid rgba(168,85,247,0.25)',
            }}
          >
            <Sparkles size={14} className="text-[#c4b5fd]" />
            <span className="text-xs uppercase tracking-[0.25em] text-[#c4b5fd] font-bold">
              Хроники
            </span>
          </div>
          <h2
            className="text-4xl sm:text-6xl font-bold mb-4 tracking-tight"
            style={{
              fontFamily: "'Satoshi', sans-serif",
              background:
                'linear-gradient(135deg, #ffffff 0%, #c4b5fd 50%, #a855f7 100%)',
              WebkitBackgroundClip: 'text',
              WebkitTextFillColor: 'transparent',
              backgroundClip: 'text',
            }}
          >
            Что нового
          </h2>
          <p className="text-white/40 text-lg max-w-xl mx-auto">
            Премьеры и анонсы, которые меняют твоё прослушивание
          </p>
        </Reveal>

        <Reveal>
          <Link
            to="/news/soundwave-release"
            className="group relative block overflow-hidden rounded-[28px] p-8 sm:p-12 no-underline transition-all duration-500"
            style={{
              background:
                'linear-gradient(135deg, rgba(168,85,247,0.10), rgba(55,196,255,0.06), rgba(255,61,122,0.06))',
              border: '1px solid rgba(168,85,247,0.25)',
              boxShadow:
                '0 30px 80px rgba(20,8,43,0.5), inset 0 1px 0 rgba(255,255,255,0.08)',
            }}
          >
            {/* Aurora glows */}
            <div
              className="absolute -top-32 -right-20 w-[500px] h-[500px] rounded-full pointer-events-none transition-opacity duration-500 opacity-50 group-hover:opacity-80"
              style={{
                background:
                  'radial-gradient(circle, rgba(168,85,247,0.55), transparent 70%)',
                filter: 'blur(60px)',
              }}
            />
            <div
              className="absolute -bottom-32 -left-20 w-[400px] h-[400px] rounded-full pointer-events-none transition-opacity duration-500 opacity-40 group-hover:opacity-70"
              style={{
                background:
                  'radial-gradient(circle, rgba(55,196,255,0.45), transparent 70%)',
                filter: 'blur(60px)',
              }}
            />
            {/* Hairline */}
            <div
              className="absolute top-0 left-0 right-0 h-px"
              style={{
                background:
                  'linear-gradient(90deg, transparent, rgba(196,181,253,0.6), rgba(55,196,255,0.6), transparent)',
              }}
            />

            <div className="relative flex flex-col md:flex-row items-start md:items-center gap-8">
              {/* Big icon block */}
              <div
                className="w-24 h-24 rounded-3xl flex items-center justify-center shrink-0 transition-transform duration-500 group-hover:scale-105"
                style={{
                  background:
                    'linear-gradient(135deg, #37c4ff, #a855f7, #ff3d7a)',
                  boxShadow:
                    '0 0 50px rgba(168,85,247,0.55), 0 20px 40px rgba(168,85,247,0.3), inset 0 1px 0 rgba(255,255,255,0.25)',
                }}
              >
                <Waves size={44} className="text-white" strokeWidth={2.2} />
              </div>

              {/* Content */}
              <div className="flex-1 min-w-0">
                <div className="flex flex-wrap items-center gap-3 mb-4">
                  <span
                    className="px-3 py-1.5 rounded-full text-[10px] font-bold tracking-[0.3em] uppercase text-white"
                    style={{
                      background:
                        'linear-gradient(135deg, rgba(168,85,247,0.4), rgba(55,196,255,0.3))',
                      border: '1px solid rgba(196,181,253,0.5)',
                    }}
                  >
                    Релиз
                  </span>
                  <span className="inline-flex items-center gap-1.5 text-[#c4b5fd] text-xs font-semibold tracking-wider">
                    <Calendar size={12} />
                    28 АПРЕЛЯ 2026
                  </span>
                </div>

                <h3
                  className="text-3xl sm:text-5xl font-bold mb-3 leading-[1.05] tracking-tight"
                  style={{ fontFamily: "'Satoshi', sans-serif" }}
                >
                  <span className="text-white/95">Саунд </span>
                  <span
                    style={{
                      background:
                        'linear-gradient(135deg, #37c4ff, #a855f7, #ff3d7a, #fbbf24)',
                      WebkitBackgroundClip: 'text',
                      WebkitTextFillColor: 'transparent',
                      backgroundClip: 'text',
                    }}
                  >
                    Волна
                  </span>
                </h3>
                <p className="text-white/55 text-base sm:text-lg leading-relaxed max-w-xl">
                  Новая эра рекомендаций. Анализ звука, понимание вайба, разбор лирики и
                  персональная лента, которая учится на твоих лайках.
                </p>

                <div className="flex items-center gap-2 mt-6 text-[#c4b5fd] text-sm font-semibold tracking-wider transition-all group-hover:gap-3">
                  <span>Открыть премьеру</span>
                  <ArrowUpRight size={16} className="transition-transform group-hover:translate-x-0.5 group-hover:-translate-y-0.5" />
                </div>
              </div>
            </div>
          </Link>
        </Reveal>

        <Reveal className="text-center mt-8">
          <Link
            to="/news"
            className="inline-flex items-center gap-2 text-white/40 hover:text-[#c4b5fd] transition-colors text-sm font-medium tracking-wider"
          >
            Все хроники
            <ArrowUpRight size={14} />
          </Link>
        </Reveal>
      </div>
    </section>
  );
}
