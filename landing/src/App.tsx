import {
  ArrowRight,
  BanIcon,
  ChevronDown,
  Cpu,
  Download,
  Globe,
  Headphones,
  Languages,
  MessageCircle,
  Music,
  ShieldOff,
  SmartphoneNfc,
  Zap,
} from 'lucide-react';
import { type ReactNode, useEffect, useRef, useState } from 'react';
import { siApple, siDebian, siFlatpak, siGithub, siLinux, siRedhat } from 'simple-icons';

/* ── Constants ── */
const RELEASES = 'https://github.com/zxcloli666/SoundCloud-Desktop/releases/latest';
const GITHUB = 'https://github.com/zxcloli666/SoundCloud-Desktop';
const DISCUSS_FEATURE = 'https://github.com/zxcloli666/SoundCloud-Desktop/discussions/121';
const DISCUSS_BUG = 'https://github.com/zxcloli666/SoundCloud-Desktop/discussions/144';
const LOGO =
  'https://raw.githubusercontent.com/zxcloli666/SoundCloud-Desktop/legacy/icons/appLogo.png';

/* ── Helpers ── */
function Si({ icon, className = '' }: { icon: { path: string }; className?: string }) {
  return (
    <svg viewBox="0 0 24 24" className={className} fill="currentColor">
      <path d={icon.path} />
    </svg>
  );
}

const siWindows = {
  path: 'M0 3.449L9.75 2.1v9.451H0m10.949-9.602L24 0v11.4H10.949M0 12.6h9.75v9.451L0 20.699M10.949 12.6H24V24l-12.9-1.801',
};

/* ── Scroll reveal ── */
function useReveal() {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const io = new IntersectionObserver(
      ([e]) => {
        if (e.isIntersecting) {
          el.classList.add('visible');
          io.unobserve(el);
        }
      },
      { threshold: 0.08, rootMargin: '0px 0px -40px 0px' },
    );
    io.observe(el);
    return () => io.disconnect();
  }, []);
  return ref;
}

function Reveal({ children, className = '' }: { children: ReactNode; className?: string }) {
  const ref = useReveal();
  return (
    <div ref={ref} className={`reveal ${className}`}>
      {children}
    </div>
  );
}

/* ══════════════════════════════════════════
   HERO
   ══════════════════════════════════════════ */
function Hero() {
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

/* ══════════════════════════════════════════
   DOWNLOAD STATS HOOK
   ══════════════════════════════════════════ */
function useDownloadCount() {
  const [count, setCount] = useState<string>('100 000+');
  useEffect(() => {
    fetch(
      'https://raw.githubusercontent.com/zxcloli666/download-history/refs/heads/main/data/zxcloli666_SoundCloud-Desktop.json',
    )
      .then((r) => r.json())
      .then((data: { total: number }[]) => {
        const total = Math.round(data.pop()?.total ?? 0);
        if (total > 0) setCount(`${total.toLocaleString('ru-RU')}+`);
      })
      .catch(() => {});
  }, []);
  return count;
}

/* ══════════════════════════════════════════
   STATS
   ══════════════════════════════════════════ */
function Stats() {
  const downloads = useDownloadCount();
  const stats = [
    { value: downloads, label: 'скачиваний' },
    { value: '~15 МБ', label: 'установщик' },
    { value: '~100 МБ', label: 'RAM' },
    { value: '60 FPS', label: 'интерфейс' },
  ];

  return (
    <section className="px-6 py-8">
      <Reveal>
        <div className="max-w-4xl mx-auto glass rounded-[28px] p-2">
          <div className="grid grid-cols-2 sm:grid-cols-4">
            {stats.map((s, i) => (
              <div
                key={s.label}
                className={`text-center py-6 px-4 ${
                  i < stats.length - 1 ? 'sm:border-r border-white/[0.06]' : ''
                } ${i < 2 ? 'border-b sm:border-b-0 border-white/[0.06]' : ''}`}
              >
                <div className="text-2xl sm:text-3xl font-bold gradient-text stat-num mb-1">
                  {s.value}
                </div>
                <div className="text-white/35 text-xs uppercase tracking-widest">{s.label}</div>
              </div>
            ))}
          </div>
        </div>
      </Reveal>
    </section>
  );
}

/* ══════════════════════════════════════════
   FEATURES
   ══════════════════════════════════════════ */
const features = [
  {
    icon: <ShieldOff size={24} />,
    title: 'Без рекламы',
    desc: 'Ноль баннеров, ноль промо-вставок, ноль всплывающих окон. Чистый интерфейс — только музыка.',
  },
  {
    icon: <BanIcon size={24} />,
    title: 'Без капчи',
    desc: 'Никаких бесконечных проверок «я не робот». Открыл — слушаешь.',
  },
  {
    icon: <Globe size={24} />,
    title: 'Доступно в России',
    desc: 'SoundCloud заблокирован — веб-версия не открывается. Приложение работает напрямую без дополнительных программ.',
  },
  {
    icon: <Music size={24} />,
    title: 'Без цензуры',
    desc: 'Полный каталог SoundCloud. Все треки, все артисты, все жанры без региональных ограничений.',
  },
  {
    icon: <Zap size={24} />,
    title: 'Нативное и лёгкое',
    desc: 'Tauri 2 + Rust вместо Electron. Установщик ~15 МБ, RAM ~100 МБ. Мгновенный запуск, 60 FPS.',
  },
  {
    icon: <Cpu size={24} />,
    title: 'Не грузит систему',
    desc: 'Минимальная нагрузка на CPU. Работает тихо в фоне, не мешает играм и другим приложениям.',
  },
  {
    icon: <SmartphoneNfc size={24} />,
    title: 'Системная интеграция',
    desc: 'Медиа-кнопки, MPRIS, Discord Rich Presence, системный трей, автообновления.',
  },
  {
    icon: <Languages size={24} />,
    title: 'Русский язык',
    desc: 'Полностью переведённый интерфейс. Язык определяется автоматически по системе.',
  },
];

function Features() {
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
          {features.map((f) => (
            <Reveal key={f.title}>
              <div className="glass feature-card p-6 h-full flex flex-col">
                <div
                  className="feature-icon text-[#ff5500] mb-5 w-10 h-10 rounded-xl flex items-center justify-center"
                  style={{ background: 'rgba(255, 85, 0, 0.08)' }}
                >
                  {f.icon}
                </div>
                <h3 className="text-[15px] font-semibold mb-2 text-white/90">{f.title}</h3>
                <p className="text-white/40 text-[13px] leading-relaxed">{f.desc}</p>
              </div>
            </Reveal>
          ))}
        </div>
      </div>
    </section>
  );
}

/* ══════════════════════════════════════════
   PLATFORMS
   ══════════════════════════════════════════ */
function Platforms() {
  const platforms = [
    { icon: siWindows, name: 'Windows', formats: '.exe  .msi', note: 'Windows 10+ / 11' },
    { icon: siDebian, name: 'Debian / Ubuntu', formats: '.deb', note: 'amd64 · arm64' },
    { icon: siRedhat, name: 'Fedora / RPM', formats: '.rpm', note: 'amd64 · arm64' },
    { icon: siLinux, name: 'Linux Universal', formats: '.AppImage', note: 'amd64 · arm64' },
    { icon: siFlatpak, name: 'Flatpak', formats: '.flatpak', note: 'amd64' },
    { icon: siApple, name: 'macOS', formats: '.dmg', note: 'Intel · Apple Silicon' },
  ];

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
          {platforms.map((p) => (
            <Reveal key={p.name}>
              <a
                href={RELEASES}
                className="glass platform-card p-5 flex items-center gap-4 no-underline text-inherit group"
              >
                <div className="text-white/50 group-hover:text-[#ff5500] transition-colors shrink-0">
                  <Si icon={p.icon} className="w-7 h-7" />
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

/* ══════════════════════════════════════════
   FAQ
   ══════════════════════════════════════════ */
function FAQ() {
  const items = [
    {
      q: 'SoundCloud заблокирован в России — как слушать?',
      a: 'Скачайте SoundCloud Desktop. Приложение работает в России без VPN и дополнительных программ. Весь каталог SoundCloud доступен полностью.',
    },
    {
      q: 'Приложение бесплатное?',
      a: 'Да, полностью бесплатное. Открытый исходный код, MIT лицензия.',
    },
    {
      q: 'Чем отличается от веб-версии?',
      a: 'Нет рекламы, нет капчи, нет региональных блокировок. Нативное приложение потребляет меньше ресурсов, интегрируется с системой — медиа-кнопки, Discord, трей.',
    },
    {
      q: 'Это безопасно?',
      a: 'Исходный код полностью открыт на GitHub — можете проверить каждую строку. Приложение не собирает данные.',
    },
    {
      q: 'Какие системные требования?',
      a: 'Windows 10+, macOS 11+, или Linux. 4 ГБ RAM, ~50 МБ места на диске. Работает даже на Raspberry Pi (arm64).',
    },
  ];

  return (
    <section className="px-6 py-20 sm:py-24" id="faq">
      <div className="max-w-3xl mx-auto">
        <Reveal className="text-center mb-16">
          <p className="text-xs uppercase tracking-[0.2em] text-[#ff5500] mb-4 font-medium">FAQ</p>
          <h2
            className="text-3xl sm:text-5xl font-bold tracking-tight"
            style={{ fontFamily: "'Satoshi', sans-serif" }}
          >
            Вопросы и ответы
          </h2>
        </Reveal>

        <div className="flex flex-col gap-3 reveal-stagger">
          {items.map((item) => (
            <Reveal key={item.q}>
              <details className="glass faq-item p-5 cursor-pointer group">
                <summary className="font-medium text-[15px] flex items-center justify-between gap-4 text-white/80">
                  {item.q}
                  <ChevronDown size={18} className="chevron text-white/20 shrink-0" />
                </summary>
                <div className="faq-answer">
                  <p className="text-white/45 text-[14px] mt-4 leading-relaxed pl-0">{item.a}</p>
                </div>
              </details>
            </Reveal>
          ))}
        </div>
      </div>
    </section>
  );
}

/* ══════════════════════════════════════════
   CTA BANNER
   ══════════════════════════════════════════ */
function CTABanner() {
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

/* ══════════════════════════════════════════
   FOOTER
   ══════════════════════════════════════════ */
function Footer() {
  return (
    <footer className="px-6 py-12">
      <div className="divider max-w-5xl mx-auto mb-12" />

      <div className="max-w-5xl mx-auto">
        <div className="flex flex-col sm:flex-row justify-between items-center gap-6 mb-10">
          <a href="#" className="flex items-center gap-3 no-underline text-inherit">
            <img src={LOGO} alt="" width={32} height={32} className="rounded-lg" />
            <span className="font-semibold text-white/70 text-sm">SoundCloud Desktop</span>
          </a>

          <nav className="flex flex-wrap gap-6 justify-center">
            {[
              { href: GITHUB, icon: <Si icon={siGithub} className="w-4 h-4" />, label: 'GitHub' },
              {
                href: DISCUSS_FEATURE,
                icon: <MessageCircle size={15} />,
                label: 'Предложить идею',
              },
              { href: DISCUSS_BUG, icon: <Headphones size={15} />, label: 'Поддержка' },
            ].map((link) => (
              <a
                key={link.label}
                href={link.href}
                className="text-white/30 hover:text-white/60 transition-colors text-xs flex items-center gap-2"
              >
                {link.icon}
                {link.label}
              </a>
            ))}
          </nav>
        </div>

        <div className="text-center text-white/15 text-xs leading-relaxed">
          <p>
            MIT License · SoundCloud — торговая марка SoundCloud Ltd. · Не аффилировано с
            SoundCloud.
          </p>
        </div>

        {/* SEO hidden text */}
        <div className="sr-only" aria-hidden="true">
          <h2>SoundCloud Desktop — скачать приложение SoundCloud для компьютера</h2>
          <p>
            SoundCloud Desktop — лучший неофициальный десктопный клиент для SoundCloud. Скачать
            SoundCloud на компьютер бесплатно. SoundCloud приложение для Windows, Linux и macOS.
            SoundCloud без рекламы и без капчи. SoundCloud в России — работает без VPN. SoundCloud
            заблокирован — альтернативный клиент. SoundCloud плеер для ПК. Музыкальный плеер
            SoundCloud Desktop. SoundCloud desktop app download free. SoundCloud client for PC.
            SoundCloud no ads no captcha. SoundCloud blocked Russia alternative.
          </p>
        </div>
      </div>
    </footer>
  );
}

/* ══════════════════════════════════════════
   APP
   ══════════════════════════════════════════ */
export function App() {
  return (
    <main>
      <Hero />
      <Stats />
      <Features />
      <Platforms />
      <FAQ />
      <CTABanner />
      <Footer />
    </main>
  );
}
