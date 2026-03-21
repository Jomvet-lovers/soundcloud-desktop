import { Module } from '@nestjs/common';
import { TypeOrmModule } from '@nestjs/typeorm';
import { AuthModule } from '../auth/auth.module.js';
import { ListeningHistory } from './entities/listening-history.entity.js';
import { HistoryController } from './history.controller.js';
import { HistoryService } from './history.service.js';

@Module({
  imports: [TypeOrmModule.forFeature([ListeningHistory]), AuthModule],
  controllers: [HistoryController],
  providers: [HistoryService],
})
export class HistoryModule {}
