export default () => ({
  port: Number.parseInt(process.env.PORT || '3000', 10),
  soundcloud: {
    clientId: process.env.SOUNDCLOUD_CLIENT_ID || '',
    clientSecret: process.env.SOUNDCLOUD_CLIENT_SECRET || '',
    redirectUri: process.env.SOUNDCLOUD_REDIRECT_URI || 'http://localhost:3000/auth/callback',
    apiBaseUrl: 'https://api.soundcloud.com',
    authBaseUrl: 'https://secure.soundcloud.com',
  },
  database: {
    host: process.env.DATABASE_HOST || 'localhost',
    port: Number.parseInt(process.env.DATABASE_PORT || '5432', 10),
    username: process.env.DATABASE_USERNAME || 'soundcloud',
    password: process.env.DATABASE_PASSWORD || 'soundcloud',
    name: process.env.DATABASE_NAME || 'soundcloud_desktop',
  },
});
