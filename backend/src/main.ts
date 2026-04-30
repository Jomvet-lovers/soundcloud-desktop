import { readFileSync } from 'node:fs';
import { ValidationPipe } from '@nestjs/common';
import { NestFactory } from '@nestjs/core';
import { FastifyAdapter, NestFastifyApplication } from '@nestjs/platform-fastify';
import { DocumentBuilder, SwaggerModule } from '@nestjs/swagger';
import { AppModule } from './app.module.js';
import { SoundcloudExceptionFilter } from './common/filters/soundcloud-exception.filter.js';

async function bootstrap() {
  // cert-runner (Rust sidecar pid-1) пишет TLS_CERT_FILE/TLS_KEY_FILE перед
  // тем как exec'нуть нас. Если их нет — TLS не используется.
  const certFile = process.env.TLS_CERT_FILE;
  const keyFile = process.env.TLS_KEY_FILE;
  const tlsOn = !!(certFile && keyFile);

  const adapter = tlsOn
    ? new FastifyAdapter({
        https: {
          cert: readFileSync(certFile!),
          key: readFileSync(keyFile!),
        },
      })
    : new FastifyAdapter();

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

  if (tlsOn) {
    const httpsPort = Number.parseInt(process.env.TLS_HTTPS_PORT ?? '443', 10);
    await app.listen(httpsPort, '0.0.0.0');
    console.log(`HTTPS server on https://0.0.0.0:${httpsPort} (cert: ${certFile})`);
    console.log(`OpenAPI spec: https://0.0.0.0:${httpsPort}/openapi.json`);
    console.log(`Swagger UI:   https://0.0.0.0:${httpsPort}/api`);
  } else {
    const port = process.env.PORT ?? 3000;
    await app.listen(port, '0.0.0.0');
    console.log(`Server running on http://localhost:${port}`);
    console.log(`OpenAPI spec: http://localhost:${port}/openapi.json`);
    console.log(`Swagger UI: http://localhost:${port}/api`);
  }
}

void bootstrap();
