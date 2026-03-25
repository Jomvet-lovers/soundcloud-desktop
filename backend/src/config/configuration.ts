export default () => ({
  port: Number.parseInt(process.env.PORT || '3000', 10),
  soundcloud: {
    clientId: process.env.SOUNDCLOUD_CLIENT_ID || '',
    clientSecret: process.env.SOUNDCLOUD_CLIENT_SECRET || '',
    redirectUri: process.env.SOUNDCLOUD_REDIRECT_URI || 'http://localhost:3000/auth/callback',
    apiBaseUrl: process.env.SC_API_PROXY_URL || 'https://api.soundcloud.com',
    authBaseUrl: process.env.SC_AUTH_PROXY_URL || 'https://secure.soundcloud.com',
    /** CF proxy URL (напр. https://images.soundcloud.su). Если задан — все запросы через X-Target */
    proxyUrl: process.env.SC_PROXY_URL || '',
  },
  database: {
    host: process.env.DATABASE_HOST || 'localhost',
    port: Number.parseInt(process.env.DATABASE_PORT || '5432', 10),
    username: process.env.DATABASE_USERNAME || 'soundcloud',
    password: process.env.DATABASE_PASSWORD || 'soundcloud',
    name: process.env.DATABASE_NAME || 'soundcloud_desktop',
  },
  cdn: {
    baseUrl: process.env.CDN_BASE_URL || '',
    authToken: process.env.CDN_AUTH_TOKEN || '',
  },
  telegram: {
    botToken: process.env.TELEGRAM_BOT_TOKEN || '',
    chatId: process.env.TELEGRAM_CHAT_ID || '',
  },
  admin: {
    token: process.env.ADMIN_TOKEN || '',
  },
});
