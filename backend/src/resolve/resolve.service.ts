import { Injectable } from '@nestjs/common';
import { SoundcloudService } from '../soundcloud/soundcloud.service.js';

@Injectable()
export class ResolveService {
  constructor(private readonly sc: SoundcloudService) {}

  resolve(token: string, url: string): Promise<unknown> {
    return this.sc.apiGet('/resolve', token, { url });
  }
}
