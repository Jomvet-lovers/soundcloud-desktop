import { ChevronDown, Download } from 'lucide-react';
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
          <span>Без цензуры</span>
          <span className="text-white/10">·</span>
          <span>Доступно в&nbsp;России</span>
        </div>

        {/* CTA */}
        <div className="flex flex-col sm:flex-row gap-4 justify-center items-center mb-14">
          <a href={RELEASES} className="btn-primary text-[17px]">
            <Download size={19} strokeWidth={2.5} />
            Скачать бесплатно
          </a>
          <a href={GITHUB} className="btn-secondary text-[17px]">
            <Si icon={siGithub} className="w-[18px] h-[18px]" />
            GitHub
          </a>
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
