import { useEffect } from 'react';
import { Link } from 'react-router-dom';
import {
  ArrowLeft,
  AudioWaveform,
  FileText,
  Music2,
  Sparkles,
  Star,
  Waves,
} from 'lucide-react';
import './SoundwaveRelease.css';

const FEATURES = [
  {
    cls: 'c1',
    num: '01',
    Icon: Music2,
    title: 'Анализ звука',
    body: 'Разбираем каждый трек на атомы — темп, тональность, энергетика, инструменты.',
  },
  {
    cls: 'c2',
    num: '02',
    Icon: Sparkles,
    title: 'Понимание вайба',
    body: 'Учитываем не только бит, но и настроение, атмосферу, эмоциональный окрас.',
  },
  {
    cls: 'c3',
    num: '03',
    Icon: FileText,
    title: 'Анализ лирики',
    body: 'Читаем тексты, понимаем смысл — грустное, мотивирующее, романтичное.',
  },
  {
    cls: 'c4',
    num: '04',
    Icon: Waves,
    title: 'Твоя волна',
    body: 'Собираем персональную ленту, которая учится на твоих лайках и скипах.',
  },
] as const;

const ROW_SIDE = ['left', 'right', 'left', 'right'] as const;

const STAR_HUES = [260, 270, 280, 290, 300, 310, 250, 200];
const STAR_ICONS = [Star, Sparkles] as const;

interface BgStar {
  i: number;
  x: number;
  y: number;
  size: number;
  hue: number;
  sat: number;
  light: number;
  opacity: number;
  blur: number;
  rotate: number;
  Icon: (typeof STAR_ICONS)[number];
  filled: boolean;
}

interface CardStar {
  i: number;
  x: number;
  y: number;
  size: number;
  hue: number;
  opacity: number;
  rotate: number;
  Icon: (typeof STAR_ICONS)[number];
  filled: boolean;
  isGold: boolean;
}

function buildBgStars(count: number, seed: number): BgStar[] {
  // deterministic LCG so SSR/CSR match and re-renders are stable
  let s = seed;
  const rand = () => {
    s = (s * 1664525 + 1013904223) >>> 0;
    return s / 0xffffffff;
  };
  const out: BgStar[] = [];
  for (let i = 0; i < count; i++) {
    const size = 8 + rand() * 22;
    const Icon = STAR_ICONS[Math.floor(rand() * STAR_ICONS.length)];
    out.push({
      i,
      x: rand() * 100,
      y: rand() * 100,
      size,
      hue: STAR_HUES[Math.floor(rand() * STAR_HUES.length)],
      sat: 70 + rand() * 25,
      light: 65 + rand() * 25,
      opacity: 0.25 + rand() * 0.55,
      blur: rand() < 0.25 ? rand() * 3 : 0,
      rotate: rand() * 360,
      Icon,
      filled: rand() < 0.55,
    });
  }
  return out;
}

function buildCardStars(count: number, seed: number): CardStar[] {
  let s = seed;
  const rand = () => {
    s = (s * 1664525 + 1013904223) >>> 0;
    return s / 0xffffffff;
  };
  const out: CardStar[] = [];
  for (let i = 0; i < count; i++) {
    const isGold = i < 6;
    const size = 10 + rand() * 18;
    out.push({
      i,
      x: 5 + rand() * 90,
      y: 5 + rand() * 90,
      size,
      hue: isGold ? 45 : STAR_HUES[Math.floor(rand() * STAR_HUES.length)],
      opacity: 0.3 + rand() * 0.5,
      rotate: rand() * 360,
      Icon: STAR_ICONS[Math.floor(rand() * STAR_ICONS.length)],
      filled: isGold || rand() < 0.6,
      isGold,
    });
  }
  return out;
}

const BG_STARS = buildBgStars(90, 0xc0ffee);
const CARD_STARS = buildCardStars(26, 0xfeed42);

export function SoundwaveRelease() {
  useEffect(() => {
    document.title = 'Премьера · Саунд Волна — SoundCloud Desktop';
  }, []);

  return (
    <div className="swave-page">
      <Link to="/news" className="swave-back-bar">
        <ArrowLeft size={14} />
        Все премьеры
      </Link>

      <div className="swave-banner">
        {/* Background stars */}
        <div className="stars">
          {BG_STARS.map((s) => {
            const Icon = s.Icon;
            const color = `hsl(${s.hue}, ${s.sat.toFixed(0)}%, ${s.light.toFixed(0)}%)`;
            return (
              <span
                key={s.i}
                className="star-glyph"
                style={{
                  left: `${s.x.toFixed(2)}%`,
                  top: `${s.y.toFixed(2)}%`,
                  color,
                  opacity: s.opacity.toFixed(2),
                  transform: `rotate(${s.rotate.toFixed(0)}deg)`,
                  filter: `drop-shadow(0 0 ${(s.size / 2).toFixed(0)}px currentColor)${s.blur ? ` blur(${s.blur.toFixed(2)}px)` : ''}`,
                }}
              >
                <Icon
                  size={s.size}
                  fill={s.filled ? 'currentColor' : 'none'}
                  strokeWidth={s.filled ? 0 : 1.5}
                />
              </span>
            );
          })}
        </div>

        <div className="sheen" />

        {/* Decorative orbits */}
        <div
          className="orbit"
          style={{ width: 1400, height: 1400, left: -200, top: -100 }}
        />
        <div
          className="orbit"
          style={{ width: 1000, height: 1000, right: -300, top: 1800 }}
        />

        {/* HERO */}
        <section className="hero">
          <div className="hero-eyebrow">Премьера · 15 мая</div>
          <h1 className="hero-title">
            <span className="hero-title-1">Встречайте</span>
            Саунд
            <br />
            Волна
          </h1>
          <p className="hero-subtitle">
            Новая <b>эра рекомендаций</b>
          </p>
        </section>

        {/* SCREENSHOT */}
        <div className="screenshot-wrap">
          <div className="screenshot-frame">
            <div className="screenshot-tag">
              <AudioWaveform size={16} strokeWidth={2.5} />
              Саунд Волна
            </div>
            <img src="/news/soundwave.webp" alt="Soundwave UI" />
          </div>
        </div>

        {/* FEATURES */}
        <section className="features">
          <div className="section-header">
            <div className="section-eyebrow">Под капотом</div>
            <h2 className="section-title">
              Как мы анализируем
              <br />
              твои вкусы
            </h2>
          </div>

          <div className="features-grid">
            {FEATURES.map((f, idx) => {
              const Icon = f.Icon;
              return (
                <div key={f.num}>
                  <div className={`feature-row ${ROW_SIDE[idx]}`}>
                    <div className={`feature-card ${f.cls}`}>
                      <div className="feature-head">
                        <span className="feature-num">{f.num}</span>
                        <div className="feature-divider" />
                        <div className="feature-icon">
                          <Icon size={22} strokeWidth={2} />
                        </div>
                      </div>
                      <div className="feature-title">{f.title}</div>
                      <div className="feature-body">{f.body}</div>
                    </div>
                  </div>
                  {idx < FEATURES.length - 1 && (
                    <div className={`arrow arrow-${idx + 1}`}>
                      <svg width="160" height="200" viewBox="0 0 160 200">
                        {ROW_SIDE[idx] === 'left' ? (
                          <>
                            <path
                              d="M 8 18 C 60 40, 120 80, 152 178"
                              strokeDasharray="3,5"
                            />
                            <path d="M 152 178 L 138 168 M 152 178 L 144 162" />
                          </>
                        ) : (
                          <>
                            <path
                              d="M 152 18 C 100 40, 40 80, 8 178"
                              strokeDasharray="3,5"
                            />
                            <path d="M 8 178 L 22 168 M 8 178 L 16 162" />
                          </>
                        )}
                      </svg>
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </section>

        {/* STAR CTA */}
        <section className="star-cta">
          <div className="star-card">
            <div className="star-card-stars">
              {CARD_STARS.map((s) => {
                const Icon = s.Icon;
                const sat = s.isGold ? 90 : 80;
                const light = s.isGold ? 65 : 75;
                const color = `hsl(${s.hue}, ${sat}%, ${light}%)`;
                return (
                  <span
                    key={s.i}
                    className="star-glyph"
                    style={{
                      left: `${s.x.toFixed(2)}%`,
                      top: `${s.y.toFixed(2)}%`,
                      color,
                      opacity: s.opacity.toFixed(2),
                      transform: `rotate(${s.rotate.toFixed(0)}deg)`,
                      filter: `drop-shadow(0 0 ${(s.size / 2).toFixed(0)}px currentColor)`,
                    }}
                  >
                    <Icon
                      size={s.size}
                      fill={s.filled ? 'currentColor' : 'none'}
                      strokeWidth={s.filled ? 0 : 1.5}
                    />
                  </span>
                );
              })}
            </div>

            <div className="star-card-icon">
              <Star size={44} fill="currentColor" strokeWidth={0} />
            </div>

            <h3 className="star-title">
              Доступно с подпиской
              <br />
              <span className="em">
                <Star size={36} fill="currentColor" strokeWidth={0} />
              </span>{' '}
              STAR
            </h3>

            <div className="price-block">
              <div className="price-label">Всего за</div>
              <div className="price-value">299 ₽</div>
            </div>

            <div className="free-banner">
              <div className="free-banner-label">Будет доступно бесплатно</div>
              <div className="free-banner-text">
                <span className="date">15 мая 2026</span>
              </div>
            </div>

            <div className="countdown-wrap">
              <img src="/news/timer-to-15may.webp" alt="Countdown timer to 15 May" />
            </div>
          </div>
        </section>

        {/* FOOTER */}
        <div className="footer">
          <div className="footer-divider" />
          <span className="brand">SoundCloud Desktop</span> · The Future of Sound
        </div>
      </div>
    </div>
  );
}
