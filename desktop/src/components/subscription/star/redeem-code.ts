import { PayError } from '../../../lib/pay-client';

/** STAR redeem code: "STAR-XXXX-XXXX-XXXX-XXXX" (pure helpers, no UI). */
export const CODE_RE = /^STAR(-[0-9A-Z]{4}){4}$/;

/** Strip to alphanumerics, drop a leading "STAR", cap to 16 body chars. */
export function normalizeBody(raw: string): string {
  let s = raw.toUpperCase().replace(/[^0-9A-Z]/g, '');
  if (s.startsWith('STAR')) s = s.slice(4);
  return s.slice(0, 16);
}

/** body (≤16) → "STAR-XXXX-XXXX-XXXX-XXXX" with progressive dashes. */
export function formatCode(body: string): string {
  const groups = body.match(/.{1,4}/g) ?? [];
  return ['STAR', ...groups].join('-');
}

/** Map a redeem error to its i18n key. */
export function redeemErrorKey(err: unknown): string {
  if (err instanceof PayError) {
    switch (err.status) {
      case 409:
        return 'starpass.redeem.errUsed';
      case 404:
        return 'starpass.redeem.errUnknown';
      case 403:
        return 'starpass.redeem.errRevoked';
      case 400:
        return 'starpass.redeem.errExpired';
    }
  }
  return 'starpass.redeem.errGeneric';
}
