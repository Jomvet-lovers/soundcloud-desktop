import type React from 'react';
import { useContext, useMemo } from 'react';
import { flushSync } from 'react-dom';
import { UNSAFE_NavigationContext as NavigationContext } from 'react-router-dom';

type NavigationContextObject = React.ContextType<typeof NavigationContext>;

interface NavigatorLike {
  push: (to: unknown, state?: unknown, options?: unknown) => void;
  replace: (to: unknown, state?: unknown, options?: unknown) => void;
}

type TransitionDocument = Document & {
  startViewTransition?: (callback: () => void) => unknown;
};

/** A navigation can touch push and replace in one go; only the first starts a
 *  transition, the rest just commit inside it. */
let transitionActive = false;

/**
 * Wraps every navigation in a View Transition — the same mechanism the tab
 * switches use (lib/view-transition.ts), applied app-wide. The router's
 * navigator is patched once, so `useNavigate`, `<Link>` and `<NavLink>` all
 * get it for free.
 *
 * React Router only honours `viewTransition` on its data router, so the
 * transition is started here: the navigation runs inside
 * `startViewTransition` and `flushSync` commits the new route before the
 * browser takes its snapshots.
 *
 * The page wrapper carries `view-transition-name: page`, so the chrome
 * (sidebar, bars) stays put while the old page crossfades into the new one.
 * Back/forward (`go`) is left alone — those animate natively.
 */
export function ViewTransitionRouter({ children }: { children: React.ReactNode }) {
  const router = useContext(NavigationContext) as NavigationContextObject;

  const value = useMemo<NavigationContextObject>(() => {
    const withTransition =
      (fn: NavigatorLike['push']) => (to: unknown, state?: unknown, options?: unknown) => {
        const doc = document as TransitionDocument;
        const reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
        if (typeof doc.startViewTransition !== 'function' || reduced || transitionActive) {
          fn(to, state, options);
          return;
        }
        transitionActive = true;
        const transition = doc.startViewTransition(() => {
          flushSync(() => fn(to, state, options));
        });
        void Promise.resolve(transition).finally(() => {
          transitionActive = false;
        });
      };
    return {
      ...router,
      navigator: {
        ...router.navigator,
        push: withTransition(router.navigator.push as NavigatorLike['push']),
        replace: withTransition(router.navigator.replace as NavigatorLike['push']),
      },
    };
  }, [router]);

  return <NavigationContext.Provider value={value}>{children}</NavigationContext.Provider>;
}
