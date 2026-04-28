import * as http from 'node:http';
import * as https from 'node:https';
import { ValidationPipe } from '@nestjs/common';
import { NestFactory } from '@nestjs/core';
import { FastifyAdapter, NestFastifyApplication } from '@nestjs/platform-fastify';
import { DocumentBuilder, SwaggerModule } from '@nestjs/swagger';
import { AppModule } from './app.module.js';
import { SoundcloudExceptionFilter } from './common/filters/soundcloud-exception.filter.js';
import { AcmeManager, buildAcmeHttpHandler, tlsConfigFromEnv } from './tls/tls.js';

async function bootstrap() {
  const tlsCfg = tlsConfigFromEnv();
  let acmeManager: AcmeManager | null = null;
  let httpServer: http.Server | null = null;
  let adapter: FastifyAdapter;

  if (tlsCfg) {
    acmeManager = new AcmeManager(tlsCfg);

    // HTTP сервер на :80 поднимаем ПЕРВЫМ — он нужен для HTTP-01 challenge
    // во время выпуска сертификата.
    httpServer = http.createServer(
      buildAcmeHttpHandler(acmeManager, tlsCfg.httpRedirect, tlsCfg.httpsPort),
    );
    await new Promise<void>((resolve, reject) => {
      httpServer!.once('error', reject);
      httpServer!.listen(tlsCfg.httpPort, '0.0.0.0', () => {
        httpServer!.off('error', reject);
        resolve();
      });
    });
    console.log(
      `[tls] HTTP listener on :${tlsCfg.httpPort} (ACME challenges${tlsCfg.httpRedirect ? ' + 301 redirect' : ''})`,
    );

    await acmeManager.start();

    adapter = new FastifyAdapter({
      serverFactory: (handler) =>
        https.createServer({ SNICallback: acmeManager!.sniCallback }, handler),
    });
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

  if (acmeManager && httpServer) {
    const httpListener = httpServer;
    const acme = acmeManager;
    app.beforeApplicationShutdown(async () => {
      acme.stop();
      await new Promise<void>((resolve) => httpListener.close(() => resolve()));
    });
  }

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
    console.log(
      `[tls] HTTPS listener on :${tlsCfg.httpsPort} for ${tlsCfg.domains.join(', ')}`,
    );
    console.log(`OpenAPI spec: https://${tlsCfg.domains[0]}/openapi.json`);
    console.log(`Swagger UI:   https://${tlsCfg.domains[0]}/api`);
  } else {
    const port = process.env.PORT ?? 3000;
    await app.listen(port, '0.0.0.0');
    console.log(`Server running on http://localhost:${port}`);
    console.log(`OpenAPI spec: http://localhost:${port}/openapi.json`);
    console.log(`Swagger UI: http://localhost:${port}/api`);
  }
}

void bootstrap();
