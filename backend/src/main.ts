import * as http from 'node:http';
import * as https from 'node:https';
import { ValidationPipe } from '@nestjs/common';
import { NestFactory } from '@nestjs/core';
import { FastifyAdapter, NestFastifyApplication } from '@nestjs/platform-fastify';
import { DocumentBuilder, SwaggerModule } from '@nestjs/swagger';
import { AppModule } from './app.module.js';
import { SoundcloudExceptionFilter } from './common/filters/soundcloud-exception.filter.js';
import { AcmeManager, buildHttpRedirectHandler, tlsConfigFromEnv } from './tls/tls.js';

async function bootstrap() {
  const tlsCfg = tlsConfigFromEnv();
  let acmeManager: AcmeManager | null = null;
  let httpServer: http.Server | null = null;
  let adapter: FastifyAdapter;

  if (tlsCfg) {
    acmeManager = new AcmeManager(tlsCfg);
    // ACME выполняется через TLS-ALPN-01 на :443 — порт 80 в критпуте не нужен.
    await acmeManager.start();

    // Fastify типизирует serverFactory под http.Server; https.Server совместим
    // по runtime, но типы расходятся — поэтому опции через any.
    const fastifyOpts: any = {
      serverFactory: (handler: http.RequestListener) =>
        https.createServer(
          {
            SNICallback: acmeManager!.sniCallback,
            ALPNCallback: acmeManager!.alpnCallback,
          },
          handler,
        ),
    };
    adapter = new FastifyAdapter(fastifyOpts);
  } else {
    adapter = new FastifyAdapter();
  }

  const app = await NestFactory.create<NestFastifyApplication>(AppModule, adapter);

  app.enableCors({
    origin: true,
    methods: ['GET', 'HEAD', 'PUT', 'PATCH', 'POST', 'DELETE', 'OPTIONS'],
    allowedHeaders: '*',
    credentials: true,
  });
  app.useGlobalPipes(
    new ValidationPipe({
      transform: true,
      whitelist: true,
      forbidNonWhitelisted: false,
    }),
  );
  app.useGlobalFilters(new SoundcloudExceptionFilter());
  app.enableShutdownHooks();

  const swaggerConfig = new DocumentBuilder()
    .setTitle('SoundCloud Desktop API')
    .setDescription(
      'Backend API for SoundCloud Desktop application. Proxies SoundCloud API with OAuth 2.1 + PKCE authentication.',
    )
    .setVersion('1.0.0')
    .addApiKey({ type: 'apiKey', name: 'x-session-id', in: 'header' }, 'session')
    .build();

  const document = SwaggerModule.createDocument(app, swaggerConfig);

  SwaggerModule.setup('api', app, document, {
    jsonDocumentUrl: '/openapi.json',
  });

  if (tlsCfg) {
    await app.listen(tlsCfg.httpsPort, '0.0.0.0');
    console.log(`[tls] HTTPS listener on :${tlsCfg.httpsPort} for ${tlsCfg.domains.join(', ')}`);
    console.log(`OpenAPI spec: https://${tlsCfg.domains[0]}/openapi.json`);
    console.log(`Swagger UI:   https://${tlsCfg.domains[0]}/api`);

    // :80 всегда поднимается. http_redirect=true → 301 на https.
    // http_redirect=false → отдаём тот же handler без TLS (mixed-mode для
    // клиентов, которые не могут в TLS).
    const fastifyInstance = app.getHttpAdapter().getInstance();
    await fastifyInstance.ready();
    const httpHandler: http.RequestListener = tlsCfg.httpRedirect
      ? buildHttpRedirectHandler(tlsCfg.httpsPort)
      : (fastifyInstance.routing.bind(fastifyInstance) as http.RequestListener);
    httpServer = http.createServer(httpHandler);
    await new Promise<void>((resolve, reject) => {
      httpServer!.once('error', reject);
      httpServer!.listen(tlsCfg.httpPort, '0.0.0.0', () => {
        httpServer!.off('error', reject);
        resolve();
      });
    });
    console.log(
      `[tls] HTTP listener on :${tlsCfg.httpPort} (${tlsCfg.httpRedirect ? '301 redirect' : 'mixed-mode passthrough'})`,
    );
  } else {
    const port = process.env.PORT ?? 3000;
    await app.listen(port, '0.0.0.0');
    console.log(`Server running on http://localhost:${port}`);
    console.log(`OpenAPI spec: http://localhost:${port}/openapi.json`);
    console.log(`Swagger UI: http://localhost:${port}/api`);
  }

  if (acmeManager) {
    const acme = acmeManager;
    const httpListener = httpServer;
    const shutdown = async () => {
      acme.stop();
      if (httpListener) {
        await new Promise<void>((resolve) => httpListener.close(() => resolve()));
      }
      await app.close();
      process.exit(0);
    };
    process.once('SIGTERM', shutdown);
    process.once('SIGINT', shutdown);
  }
}

void bootstrap();
