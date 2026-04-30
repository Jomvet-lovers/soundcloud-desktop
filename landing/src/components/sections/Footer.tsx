import { Headphones, Mail, MessageCircle, MessageSquare, Ticket } from 'lucide-react';
import { siGithub } from 'simple-icons';
import {
  DISCORD,
  DISCUSS_BUG,
  DISCUSS_FEATURE,
  GITHUB,
  LOGO,
  PRIVACY_URL,
  SUPPORT_EMAIL,
  TERMS_URL,
} from '../../constants';
import { Si } from '../ui/Si';

export function Footer() {
  return (
    <footer className="px-6 py-12">
      <div className="divider max-w-5xl mx-auto mb-12" />

      <div className="max-w-5xl mx-auto">
        <div className="flex flex-col items-center gap-3 mb-8 text-center">
          <span className="text-white/40 text-xs uppercase tracking-wider">Тех. поддержка</span>
          <div className="flex flex-wrap gap-x-6 gap-y-2 justify-center items-center">
            <a
              href={`mailto:${SUPPORT_EMAIL}`}
              className="text-white/70 hover:text-white transition-colors text-sm flex items-center gap-2"
            >
              <Mail size={15} />
              {SUPPORT_EMAIL}
            </a>
            <a
              href={DISCORD}
              target="_blank"
              rel="noopener noreferrer"
              className="text-white/70 hover:text-white transition-colors text-sm flex items-center gap-2"
            >
              <Ticket size={15} />
              Создать тикет
            </a>
            <a
              href={DISCUSS_BUG}
              target="_blank"
              rel="noopener noreferrer"
              className="text-white/70 hover:text-white transition-colors text-sm flex items-center gap-2"
            >
              <Headphones size={15} />
              Отправить проблему
            </a>
          </div>
        </div>

        <div className="flex flex-col sm:flex-row justify-between items-center gap-6 mb-10">
          <a href="#" className="flex items-center gap-3 no-underline text-inherit">
            <img src={LOGO} alt="" width={32} height={32} className="rounded-lg" />
            <span className="font-semibold text-white/70 text-sm">SoundCloud Desktop</span>
          </a>

          <nav className="flex flex-wrap gap-6 justify-center">
            {[
              { href: GITHUB, icon: <Si icon={siGithub} className="w-4 h-4" />, label: 'GitHub' },
              { href: DISCORD, icon: <MessageSquare size={15} />, label: 'Discord' },
              {
                href: DISCUSS_FEATURE,
                icon: <MessageCircle size={15} />,
                label: 'Предложить идею',
              },
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

        <div className="text-center text-white/50 text-xs leading-relaxed space-y-2">
          <p>
            MIT License · SoundCloud — торговая марка SoundCloud Ltd. · Не аффилировано с
            SoundCloud.
          </p>
          <div className="flex gap-4 justify-center">
            <a href={TERMS_URL} className="hover:text-white/80 transition-colors">
              Пользовательское соглашение
            </a>
            <span>·</span>
            <a href={PRIVACY_URL} className="hover:text-white/80 transition-colors">
              Политика конфиденциальности
            </a>
          </div>
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
