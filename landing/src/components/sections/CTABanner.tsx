import { Download } from 'lucide-react';
import { RELEASES } from '../../constants';
import { Reveal } from '../ui/Reveal';

export function CTABanner() {
  return (
    <section className="px-6 pb-20">
      <Reveal>
        <div className="max-w-4xl mx-auto relative overflow-hidden rounded-[28px]">
          {/* Background glow */}
          <div className="absolute inset-0 bg-gradient-to-br from-[#ff5500]/10 via-transparent to-[#ff3300]/5" />
          <div className="orb orb-glow w-[300px] h-[300px] bg-[#ff5500] -top-[100px] -right-[100px]" />

          <div
            className="relative z-10 p-10 sm:p-16 text-center"
            style={{
              background: 'rgba(255,255,255,0.02)',
              border: '1px solid rgba(255,255,255,0.06)',
              borderRadius: '28px',
            }}
          >
            <h2
              className="text-2xl sm:text-4xl font-bold mb-4 tracking-tight"
              style={{ fontFamily: "'Satoshi', sans-serif" }}
            >
              Слушай музыку <span className="gradient-text">без ограничений</span>
            </h2>
            <p className="text-white/40 mb-8 max-w-md mx-auto">
              100 000+ пользователей уже перешли на SoundCloud Desktop
            </p>
            <a href={RELEASES} className="btn-primary text-lg">
              <Download size={19} strokeWidth={2.5} />
              Скачать бесплатно
            </a>
          </div>
        </div>
      </Reveal>
    </section>
  );
}
