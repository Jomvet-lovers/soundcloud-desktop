export const RELEASES = 'https://github.com/zxcloli666/SoundCloud-Desktop/releases/latest';
export const GITHUB = 'https://github.com/zxcloli666/SoundCloud-Desktop';
export const DISCUSS_FEATURE = 'https://github.com/zxcloli666/SoundCloud-Desktop/discussions/121';
export const DISCUSS_BUG = 'https://github.com/zxcloli666/SoundCloud-Desktop/issues/new';
export const DISCORD = 'https://discord.gg/xQcGBP8fGG';
export const SUPPORT_EMAIL = 'support@soundcloud.su';
export const LOGO = '/favicon.png';

export const TERMS_URL = '/terms';
export const PRIVACY_URL = '/privacy';
export const NEWS_URL = '/news';

export const siWindows = {
  path: 'M0 3.449L9.75 2.1v9.451H0m10.949-9.602L24 0v11.4H10.949M0 12.6h9.75v9.451L0 20.699M10.949 12.6H24V24l-12.9-1.801',
};

export const features = [
  {
    title: 'Без рекламы',
    desc: 'Ноль баннеров, ноль промо-вставок, ноль всплывающих окон. Чистый интерфейс — только музыка.',
  },
  {
    title: 'Без капчи',
    desc: 'Никаких бесконечных проверок «я не робот». Открыл — слушаешь.',
  },
  {
    title: 'Доступно в России',
    desc: 'Пользуйтесь приложением даже во время ограничений!',
  },
  {
    title: 'Полный каталог',
    desc: 'Все треки, все артисты, все жанры SoundCloud без региональных ограничений.',
  },
  {
    title: 'Нативное и лёгкое',
    desc: 'Tauri 2 + Rust вместо Electron. Установщик ~15 МБ, RAM ~100 МБ. Мгновенный запуск, 60 FPS.',
  },
  {
    title: 'Не грузит систему',
    desc: 'Минимальная нагрузка на CPU. Работает тихо в фоне, не мешает играм и другим приложениям.',
  },
  {
    title: 'Системная интеграция',
    desc: 'Медиа-кнопки, MPRIS, Discord Rich Presence, системный трей, автообновления.',
  },
  {
    title: 'Русский язык',
    desc: 'Полностью переведённый интерфейс. Язык определяется автоматически по системе.',
  },
];

export const platforms = [
  { name: 'Windows', formats: '.exe  .msi', note: 'Windows 10+ / 11' },
  { name: 'Debian / Ubuntu', formats: '.deb', note: 'amd64 · arm64' },
  { name: 'Fedora / RPM', formats: '.rpm', note: 'amd64 · arm64' },
  { name: 'Linux Universal', formats: '.AppImage', note: 'amd64 · arm64' },
  { name: 'Flatpak', formats: '.flatpak', note: 'amd64' },
  { name: 'macOS', formats: '.dmg', note: 'Intel · Apple Silicon' },
];

export const faqItems = [
  {
    q: 'SoundCloud заблокирован в России — как слушать?',
    a: 'Скачайте SoundCloud Desktop. Приложение работает в России без ограничений. Весь каталог SoundCloud доступен полностью.',
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
