import type { Route } from './router';

// глубокие компоненты (оверлеи, модалки) зовут go() без prop-drilling; хендлер ставит роутер
let handler: ((route: Route) => void) | null = null;

export function setNavHandler(fn: ((route: Route) => void) | null) {
  handler = fn;
  if (typeof window !== 'undefined') (window as unknown as { __scGo?: typeof go }).__scGo = fn ? go : undefined;
}

export function go(route: Route) {
  handler?.(route);
}
