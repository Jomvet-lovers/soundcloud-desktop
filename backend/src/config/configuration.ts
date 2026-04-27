export default () => ({
  port: Number.parseInt(process.env.PORT || '3000', 10),
  soundcloud: {
    clientId: process.env.SOUNDCLOUD_CLIENT_ID || '',
    clientSecret: process.env.SOUNDCLOUD_CLIENT_SECRET || '',
    redirectUri: process.env.SOUNDCLOUD_REDIRECT_URI || 'http://localhost:3000/auth/callback',
    proxyUrl: process.env.SC_PROXY_URL || '',
    proxyFallback: process.env.SC_PROXY_FALLBACK === 'true',
  },
  database: {
    host: process.env.DATABASE_HOST || 'localhost',
    port: Number.parseInt(process.env.DATABASE_PORT || '5432', 10),
    username: process.env.DATABASE_USERNAME || 'soundcloud',
    password: process.env.DATABASE_PASSWORD || 'soundcloud',
    name: process.env.DATABASE_NAME || 'soundcloud_desktop',
  },
  streaming: {
    serviceUrl: process.env.STREAMING_SERVICE_URL || 'http://localhost:8080',
  },
  subscriptions: {
    snapshotDir: process.env.SUBSCRIPTIONS_SNAPSHOT_DIR || '/snapshots',
  },
  telegram: {
    botToken: process.env.TELEGRAM_BOT_TOKEN || '',
    chatId: process.env.TELEGRAM_CHAT_ID || '',
  },
  admin: {
    token: process.env.ADMIN_TOKEN || '',
  },
  qdrant: {
    url: process.env.QDRANT_URL || 'http://localhost:6333',
    apiKey: process.env.QDRANT_API_KEY || '',
  },
  nats: {
    url: process.env.NATS_URL || 'nats://localhost:4222',
  },
  redis: {
    url: process.env.REDIS_URL || 'redis://localhost:6379',
  },
  storage: {
    url: process.env.STORAGE_URL || 'http://localhost:3002',
  },
  internal: {
    token: process.env.INTERNAL_TOKEN || '',
  },
});
