// Issue #578: a `matchMedia` mock for jsdom that understands `min-width` /
// `max-width` queries against a settable viewport width.
let width = 1280;
const listeners = new Set<() => void>();

function evaluate(query: string): boolean {
  const checks = [...query.matchAll(/\((min|max)-width:\s*([\d.]+)px\)/g)];
  if (checks.length === 0) return false;
  return checks.every(([, kind, px]) => (kind === "min" ? width >= Number(px) : width <= Number(px)));
}

export function installMatchMediaMock(): void {
  if (typeof window === "undefined") return; // node-environment tests
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    writable: true,
    value: (query: string): MediaQueryList => {
      let last = evaluate(query);
      const own = new Set<EventListenerOrEventListenerObject>();
      const notify = () => {
        const now = evaluate(query);
        if (now === last) return;
        last = now;
        for (const l of own) typeof l === "function" ? l({ matches: now, media: query } as never) : l.handleEvent({ matches: now, media: query } as never);
      };
      listeners.add(notify);
      return {
        get matches() {
          return evaluate(query);
        },
        media: query,
        onchange: null,
        addEventListener: (_t: string, l: EventListenerOrEventListenerObject) => void own.add(l),
        removeEventListener: (_t: string, l: EventListenerOrEventListenerObject) => void own.delete(l),
        addListener: () => {},
        removeListener: () => {},
        dispatchEvent: () => false,
      } as MediaQueryList;
    },
  });
}

/** Removes `matchMedia`, to test the "not supported" path. */
export function removeMatchMedia(): void {
  Object.defineProperty(window, "matchMedia", { configurable: true, writable: true, value: undefined });
}

/** Sets the mocked window width in px and notifies listeners whose result changed. */
export function setViewportWidth(px: number): void {
  width = px;
  for (const n of [...listeners]) n();
}

/** `setViewportWidth(390)`: a phone-sized window. */
export const setNarrow = () => setViewportWidth(390);
/** `setViewportWidth(1280)`: the default desktop width. */
export const setWide = () => setViewportWidth(1280);
