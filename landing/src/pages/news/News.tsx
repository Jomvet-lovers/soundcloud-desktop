import { useEffect } from 'react';
import { Link } from 'react-router-dom';
import { ArrowLeft, ArrowUpRight, Calendar, Sparkles, Waves } from 'lucide-react';

interface NewsEntry {
  slug: string;
  date: string;
  badge: string;
  title: string;
  desc: string;
  Icon: typeof Waves;
  accent: { from: string; to: string; glow: string };
}

const ENTRIES: NewsEntry[] = [
  {
    slug: 'soundwave-release',
    date: '28 апреля 2026',
    badge: 'Релиз',
    title: 'Саунд Волна — новая эра рекомендаций',
    desc: 'Анализ звука, понимание вайба, разбор лирики и персональная лента, которая учится на твоих лайках.',
    Icon: Waves,
    accent: {
      from: '#37c4ff',
      to: '#a855f7',
      glow: 'rgba(168,85,247,0.45)',
    },
  },
];

export function News() {
  useEffect(() => {
    document.title = 'Хроники — SoundCloud Desktop';
  }, []);

  return (
    <div className="min-h-screen bg-[#050507] text-white/90 px-6 py-20 relative overflow-hidden">
      {/* Ambient orbs */}
      <div className="orb orb-glow-lg w-[700px] h-[700px] bg-[#a855f7] -top-[200px] -left-[200px]" />
      <div className="orb orb-glow w-[500px] h-[500px] bg-[#37c4ff] top-[40%] right-[10%]" />
      <div className="orb orb-glow w-[400px] h-[400px] bg-[#ff3d7a] bottom-[10%] left-[20%]" />

      <div className="max-w-5xl mx-auto relative z-10">
        <Link
          to="/"
          className="inline-flex items-center gap-2 text-white/40 hover:text-white/80 transition-colors text-sm mb-12"
        >
          <ArrowLeft size={14} />
          На главную
        </Link>

        {/* Header */}
        <div className="text-center mb-16">
          <div
            className="inline-flex items-center gap-2 px-4 py-2 rounded-full mb-6"
            style={{
              background: 'linear-gradient(135deg, rgba(168,85,247,0.15), rgba(55,196,255,0.08))',
              border: '1px solid rgba(168,85,247,0.25)',
            }}
          >
            <Sparkles size={14} className="text-[#c4b5fd]" />
            <span className="text-xs uppercase tracking-[0.25em] text-[#c4b5fd] font-bold">
              Что нового
            </span>
          </div>
          <h1
            className="text-5xl sm:text-7xl font-bold mb-6 tracking-tight"
            style={{
              fontFamily: "'Satoshi', sans-serif",
              background:
                'linear-gradient(135deg, #ffffff 0%, #c4b5fd 50%, #a855f7 100%)',
              WebkitBackgroundClip: 'text',
              WebkitTextFillColor: 'transparent',
              backgroundClip: 'text',
            }}
          >
            Хроники
          </h1>
          <p className="text-white/50 text-lg max-w-xl mx-auto leading-relaxed">
            Премьеры, обновления и анонсы — всё, что движет SoundCloud Desktop вперёд.
          </p>
        </div>

        {/* Entries */}
        <div className="grid gap-6">
          {ENTRIES.map((e) => {
            const Icon = e.Icon;
            return (
              <Link
                key={e.slug}
                to={`/news/${e.slug}`}
                className="group relative overflow-hidden rounded-3xl p-8 sm:p-10 transition-all duration-500 no-underline"
                style={{
                  background:
                    'linear-gradient(135deg, rgba(255,255,255,0.04), rgba(255,255,255,0.01))',
                  border: '1px solid rgba(255,255,255,0.08)',
                  boxShadow: `0 20px 60px rgba(20,8,43,0.4)`,
                }}
              >
                {/* Accent glow */}
                <div
                  className="absolute -top-20 -right-20 w-[300px] h-[300px] rounded-full pointer-events-none transition-opacity duration-500 opacity-40 group-hover:opacity-70"
                  style={{
                    background: `radial-gradient(circle, ${e.accent.glow}, transparent 70%)`,
                    filter: 'blur(40px)',
                  }}
                />
                {/* Top hairline */}
                <div
                  className="absolute top-0 left-0 right-0 h-px"
                  style={{
                    background: `linear-gradient(90deg, transparent, ${e.accent.from}88, ${e.accent.to}88, transparent)`,
                  }}
                />

                <div className="relative flex flex-col sm:flex-row gap-6 sm:items-center">
                  {/* Icon */}
                  <div
                    className="w-20 h-20 rounded-2xl flex items-center justify-center shrink-0 transition-transform duration-500 group-hover:scale-110"
                    style={{
                      background: `linear-gradient(135deg, ${e.accent.from}, ${e.accent.to})`,
                      boxShadow: `0 0 40px ${e.accent.glow}, inset 0 1px 0 rgba(255,255,255,0.2)`,
                    }}
                  >
                    <Icon size={36} className="text-white" strokeWidth={2} />
                  </div>

                  {/* Body */}
                  <div className="flex-1 min-w-0">
                    <div className="flex flex-wrap items-center gap-3 mb-3">
                      <span
                        className="px-3 py-1 rounded-full text-[10px] font-bold tracking-[0.25em] uppercase"
                        style={{
                          background: `linear-gradient(135deg, ${e.accent.from}33, ${e.accent.to}22)`,
                          border: `1px solid ${e.accent.from}55`,
                          color: '#fff',
                        }}
                      >
                        {e.badge}
                      </span>
                      <span className="inline-flex items-center gap-1.5 text-white/40 text-xs">
                        <Calendar size={12} />
                        {e.date}
                      </span>
                    </div>
                    <h2
                      className="text-2xl sm:text-3xl font-bold mb-2 text-white/95 tracking-tight transition-colors group-hover:text-white"
                      style={{ fontFamily: "'Satoshi', sans-serif" }}
                    >
                      {e.title}
                    </h2>
                    <p className="text-white/50 text-[15px] leading-relaxed">{e.desc}</p>
                  </div>

                  {/* Arrow */}
                  <div
                    className="hidden sm:flex w-12 h-12 rounded-full items-center justify-center shrink-0 transition-all duration-500 group-hover:translate-x-1 group-hover:-translate-y-1"
                    style={{
                      background: 'rgba(255,255,255,0.05)',
                      border: '1px solid rgba(255,255,255,0.1)',
                      color: '#c4b5fd',
                    }}
                  >
                    <ArrowUpRight size={20} />
                  </div>
                </div>
              </Link>
            );
          })}
        </div>
      </div>
    </div>
  );
}
