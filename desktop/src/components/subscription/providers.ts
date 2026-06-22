import type { CheckoutReq, PlansResponse, PlategaMethod, ProviderId } from '../../lib/pay-client';

// A user-facing "activation method" flattens the pay backend's
// provider × method matrix into the five concrete options the concept shows.
export type ActivationKind = 'sbp' | 'card_ru' | 'card_intl' | 'crypto' | 'tgstars';

export interface ActivationOption {
  kind: ActivationKind;
  provider: ProviderId;
  method?: PlategaMethod;
  /** i18n suffix under starpass.method.* */
  i18n: string;
  /** mono tag, e.g. "TON / USDT" */
  tag: string;
}

const ORDER: Omit<ActivationOption, 'tag'>[] = [
  { kind: 'sbp', provider: 'platega', method: 'sbp', i18n: 'sbp' },
  { kind: 'card_ru', provider: 'platega', method: 'card_ru', i18n: 'cardRu' },
  { kind: 'card_intl', provider: 'platega', method: 'card_intl', i18n: 'cardIntl' },
  { kind: 'crypto', provider: 'cryptobot', i18n: 'crypto' },
  { kind: 'tgstars', provider: 'tgstars', i18n: 'tgStars' },
];

const TAGS: Record<ActivationKind, string> = {
  sbp: 'NSPK',
  card_ru: '3-D SECURE',
  card_intl: 'USD / EUR',
  crypto: 'TON / USDT',
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
