import type { CheckoutReq, PlansResponse, PlategaMethod, ProviderId } from '../../lib/pay-client';

// A user-facing "activation method" flattens the pay backend's
// provider × method matrix into the five concrete options the concept shows.
export type ActivationKind =
  | 'sbp'
  | 'card_ru'
  | 'card_intl'
  | 'crypto_platega'
  | 'crypto_bot'
  | 'tgstars';

export interface ActivationOption {
  kind: ActivationKind;
  provider: ProviderId;
  method?: PlategaMethod;
  /** i18n suffix under starpass.method.* */
  i18n: string;
  /** mono tag, e.g. "TON / USDT" */
  tag: string;
  /**
   * Whether this method can auto-renew. The recurring toggle is shown ONLY for
   * options that support it (data-driven — add a new recurring method here and
   * it surfaces on its own; no hardcoded "TG Stars only" copy anywhere).
   */
  recurring: boolean;
}

const ORDER: Omit<ActivationOption, 'tag'>[] = [
  { kind: 'sbp', provider: 'platega', method: 'sbp', i18n: 'sbp', recurring: false },
  { kind: 'card_ru', provider: 'platega', method: 'card_ru', i18n: 'cardRu', recurring: false },
  {
    kind: 'card_intl',
    provider: 'platega',
    method: 'card_intl',
    i18n: 'cardIntl',
    recurring: false,
  },
  // Two crypto rails: Platega = pay in crypto on-chain (crypto as crypto);
  // CryptoBot = a hot wallet for users who already hold a balance in the bot.
  {
    kind: 'crypto_platega',
    provider: 'platega',
    method: 'crypto',
    i18n: 'cryptoPlatega',
    recurring: false,
  },
  { kind: 'crypto_bot', provider: 'cryptobot', i18n: 'cryptoBot', recurring: false },
  { kind: 'tgstars', provider: 'tgstars', i18n: 'tgStars', recurring: true },
];

const TAGS: Record<ActivationKind, string> = {
  sbp: 'NSPK',
  card_ru: '3-D SECURE',
  card_intl: 'USD / EUR',
  crypto_platega: 'ON-CHAIN',
  crypto_bot: '@CryptoBot',
  tgstars: '★',
};

/** Build the available activation options from a plans() response. */
export function activationOptions(plans: PlansResponse): ActivationOption[] {
  const plategaMethods = new Set<PlategaMethod>(plans.methods.platega);
  return ORDER.filter((o) => {
    if (!plans.providers[o.provider]) return false;
    if (o.provider === 'platega' && o.method) return plategaMethods.has(o.method);
    return true;
  }).map((o) => ({ ...o, tag: TAGS[o.kind] }));
}

/** Map an activation option + plan into a checkout request body. */
export function toCheckout(
  opt: ActivationOption,
  planId: string,
  recurring?: boolean,
): CheckoutReq {
  return {
    plan_id: planId,
    provider: opt.provider,
    method: opt.method,
    recurring,
  };
}
