import { useCallback, useState } from 'react';

export type Route =
  | { name: 'home' }
  | { name: 'search' }
  | { name: 'discover' }
  | { name: 'library' }
  | { name: 'offline' }
  | { name: 'settings' }
  | { name: 'star' }
  | { name: 'history' }
  | { name: 'track'; urn: string };

/**
 * Внутренний роутер приложения — платформенно нейтральный (не привязан к
 * веб-истории/react-router): десктоп/мобайл заведут свои реализации в Ф3-Ф5,
 * страницы-конструктор от роутера не зависят.
 */
export function useRouter(initial: Route = { name: 'home' }) {
  const [stack, setStack] = useState<Route[]>([initial]);
  const [pos, setPos] = useState(0);

  const navigate = useCallback((route: Route) => {
    setStack((s) => [...s.slice(0, pos + 1), route]);
    setPos((p) => p + 1);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pos]);

  const back = useCallback(() => setPos((p) => Math.max(0, p - 1)), []);
  const forward = useCallback((s = stack) => setPos((p) => Math.min(s.length - 1, p + 1)), [stack]);

  return {
    route: stack[pos],
    canBack: pos > 0,
    canForward: pos < stack.length - 1,
    navigate,
    back,
    forward: () => forward(),
  };
}

export type Router = ReturnType<typeof useRouter>;
