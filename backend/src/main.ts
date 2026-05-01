import { ValidationPipe } from '@nestjs/common';
import { NestFactory } from '@nestjs/core';
import { FastifyAdapter, NestFastifyApplication } from '@nestjs/platform-fastify';
import { DocumentBuilder, SwaggerModule } from '@nestjs/swagger';
import { AppModule } from './app.module.js';
import { SoundcloudExceptionFilter } from './common/filters/soundcloud-exception.filter.js';

async function bootstrap() {
  const adapter = new FastifyAdapter({ trustProxy: true });
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
  SwaggerModule.setup('api', app, document, { jsonDocumentUrl: '/openapi.json' });

  await app.init();
  const fastify = app.getHttpAdapter().getInstance();

  const sock = process.env.BACKEND_SOCKET;
  if (sock) {
    await (fastify.listen as (opts: { path: string }) => Promise<string>)({ path: sock });
    console.log(`Listening on UDS ${sock} (worker=${process.env.BACKEND_INDEX ?? '?'})`);
  } else {
    const port = Number.parseInt(String(process.env.PORT ?? '3000'), 10);
    await fastify.listen({ port, host: '0.0.0.0' });
    console.log(`Listening on http://0.0.0.0:${port}`);
    console.log(`OpenAPI: http://0.0.0.0:${port}/openapi.json`);
    console.log(`Swagger: http://0.0.0.0:${port}/api`);
  }
}

void bootstrap();
