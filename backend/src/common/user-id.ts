import { createHash } from 'node:crypto';

/**
 * Детерминированный маппинг scUserId → number для Qdrant point id.
 * Используется во всех коллекциях user_taste_*, чтобы один юзер имел
 * один и тот же point id во всех векторных пространствах.
 */
export function userIdToQdrantId(userId: string): number {
  const hash = createHash('sha256').update(userId).digest();
  return Number(hash.readBigUInt64BE(0) % BigInt(Number.MAX_SAFE_INTEGER));
}
