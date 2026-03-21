import { Module } from '@nestjs/common';
import { TypeOrmModule } from '@nestjs/typeorm';
import { AuthModule } from '../auth/auth.module.js';
import { LocalLike } from './entities/local-like.entity.js';
import { LocalLikesController } from './local-likes.controller.js';
import { LocalLikesService } from './local-likes.service.js';

@Module({
  imports: [TypeOrmModule.forFeature([LocalLike]), AuthModule],
  controllers: [LocalLikesController],
  providers: [LocalLikesService],
  exports: [LocalLikesService],
})
export class LocalLikesModule {}
