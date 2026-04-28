import { ChevronDown, Download, Sparkles, Star } from 'lucide-react';
import { siApple, siGithub, siLinux } from 'simple-icons';
import { GITHUB, LOGO, RELEASES, siWindows } from '../../constants';
import { Si } from '../ui/Si';

export function Hero() {
  return (
    <section className="relative min-h-screen flex items-center justify-center px-6 overflow-hidden">
      {/* Ambient lighting */}
      <div className="orb orb-glow-lg w-[700px] h-[700px] bg-[#ff5500] -top-[200px] -left-[200px]" />
      <div className="orb orb-glow-lg w-[600px] h-[600px] bg-[#ff3300] -bottom-[150px] -right-[150px]" />
      <div className="orb orb-glow w-[400px] h-[400px] bg-[#ff7700] top-[35%] left-[55%]" />
      {/* Subtle grid */}
      <div
        className="absolute inset-0 pointer-events-none opacity-[0.03]"
        style={{
          backgroundImage:
            'linear-gradient(rgba(255,255,255,0.1) 1px, transparent 1px), linear-gradient(90deg, rgba(255,255,255,0.1) 1px, transparent 1px)',
          backgroundSize: '80px 80px',
        }}
      />

      <div className="relative z-10 text-center max-w-4xl mx-auto">
        {/* App icon with glow ring */}
        <div className="mb-10 inline-block icon-ring">
          <img
            src={LOGO}
            alt="SoundCloud Desktop"
            width={110}
            height={110}
            className="rounded-[26px] relative z-10"
            style={{ boxShadow: '0 20px 60px rgba(255, 85, 0, 0.2)' }}
          />
        </div>

        <h1
          className="text-[clamp(3rem,8vw,6rem)] font-bold leading-[1.05] tracking-tight mb-6"
          style={{ fontFamily: "'Satoshi', sans-serif" }}
        >
          <span className="gradient-text">SoundCloud</span>
          <br />
          <span className="text-white/90">Desktop</span>
        </h1>

        <p className="text-xl sm:text-2xl text-white/50 mb-3 max-w-2xl mx-auto leading-relaxed font-light">
          Нативное десктопное приложение для SoundCloud
        </p>

        <div className="flex flex-wrap gap-x-2 gap-y-1 justify-center text-sm text-white/30 mb-10">
          <span>Без рекламы</span>
          <span className="text-white/10">·</span>
          <span>Без капчи</span>
          <span className="text-white/10">·</span>
          <span>Полный каталог</span>
          <span className="text-white/10">·</span>
          <span>Доступно в&nbsp;России</span>
        </div>

        {/* CTA */}
        <div className="flex flex-col gap-4 justify-center items-center mb-14">
          <div className="flex flex-col sm:flex-row gap-4">
            <a href={RELEASES} className="btn-primary text-[17px]">
              <Download size={19} strokeWidth={2.5} />
              Скачать бесплатно
            </a>
            <a href={GITHUB} className="btn-secondary text-[17px]">
              <Si icon={siGithub} className="w-[18px] h-[18px]" />
              GitHub
            </a>
          </div>
          <div className="flex flex-col sm:flex-row gap-3">
            <a
              href="#star"
            className="relative overflow-hidden px-6 py-3 rounded-2xl text-[17px] font-medium transition-all hover:scale-105 flex items-center gap-2"
            style={{
              background: 'linear-gradient(135deg, rgba(139,92,246,0.25), rgba(168,85,247,0.2))',
              border: '1px solid rgba(168,85,247,0.4)',
              boxShadow: '0 0 30px rgba(139,92,246,0.3), inset 0 1px 0 rgba(255,255,255,0.1)',
            }}
          >
            <div className="absolute inset-0 opacity-30">
              {[...Array(8)].map((_, i) => (
                <Star
                  key={i}
                  size={12 + (i % 3) * 4}
                  fill="currentColor"
                  className="absolute text-amber-400/40"
                  style={{
                    left: `${(i * 23) % 90}%`,
                    top: `${(i * 37) % 80}%`,
                    animation: `star-float ${3 + (i % 3)}s ease-in-out ${i * 0.3}s infinite alternate`,
                  }}
                />
              ))}
            </div>
            <Star size={18} fill="currentColor" className="text-amber-400 relative z-10" />
            <span className="relative z-10 gradient-text">Подписка Star</span>
          </a>
          <a
            href="#news"
            className="relative overflow-hidden px-6 py-3 rounded-2xl text-[17px] font-medium transition-all hover:scale-105 flex items-center gap-2"
            style={{
              background:
                'linear-gradient(135deg, rgba(55,196,255,0.18), rgba(168,85,247,0.18), rgba(255,61,122,0.14))',
              border: '1px solid rgba(196,181,253,0.4)',
              boxShadow:
                '0 0 30px rgba(168,85,247,0.25), inset 0 1px 0 rgba(255,255,255,0.1)',
            }}
          >
            <div className="absolute inset-0 opacity-25">
              {[...Array(6)].map((_, i) => (
                <Sparkles
                  key={i}
                  size={10 + (i % 3) * 3}
                  className="absolute text-[#c4b5fd]"
                  style={{
                    left: `${(i * 31) % 85}%`,
                    top: `${(i * 41) % 75}%`,
                    animation: `star-float ${3 + (i % 3)}s ease-in-out ${i * 0.4}s infinite alternate`,
                  }}
                />
              ))}
            </div>
            <Sparkles size={18} className="text-[#c4b5fd] relative z-10" />
            <span
              className="relative z-10"
              style={{
                background:
                  'linear-gradient(135deg, #c4b5fd, #ffffff, #fbbf24)',
                WebkitBackgroundClip: 'text',
                WebkitTextFillColor: 'transparent',
                backgroundClip: 'text',
                fontWeight: 600,
              }}
            >
              Хроники
            </span>
          </a>
          </div>
        </div>

        {/* Platform pills */}
        <div className="flex flex-wrap gap-3 justify-center">
          {[
            { icon: siWindows, label: 'Windows' },
            { icon: siLinux, label: 'Linux' },
            { icon: siApple, label: 'macOS' },
          ].map(({ icon, label }) => (
            <div
              key={label}
              className="flex items-center gap-2 px-4 py-2 rounded-full text-xs text-white/40"
              style={{
                background: 'rgba(255,255,255,0.03)',
                border: '1px solid rgba(255,255,255,0.06)',
              }}
            >
              <Si icon={icon} className="w-3.5 h-3.5" />
              {label}
            </div>
          ))}
        </div>

        {/* Scroll hint */}
        <div className="mt-16 w-full flex justify-center animate-bounce opacity-20">
          <ChevronDown size={24} />
        </div>
      </div>
    </section>
  );
}
