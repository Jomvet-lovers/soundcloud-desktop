import { createParamDecorator, type ExecutionContext } from '@nestjs/common';

export const SessionId = createParamDecorator((_data: unknown, ctx: ExecutionContext): string => {
  const request = ctx.switchToHttp().getRequest();
  return request.sessionId;
});
