/** Shared in-app event bus (issue #905). Delivery iterates a snapshot of the
 * listeners, so subscribing or unsubscribing during delivery is safe: a
 * listener added during a publish does not receive that event, and one that
 * unsubscribes does not stop delivery to the others.
 *
 * With `replayLast`, a value published while nobody is subscribed is kept and
 * handed to the next subscriber exactly once (then cleared), so an event
 * raised before its host component mounted is not lost. */

export interface Bus<T> {
  publish(value: T): void;
  /** Returns an idempotent unsubscribe function (React `useEffect` cleanup). */
  subscribe(listener: (value: T) => void): () => void;
}

export function createBus<T>(options?: { replayLast?: boolean }): Bus<T> {
  const replayLast = options?.replayLast === true;
  const listeners = new Set<(value: T) => void>();
  let pending: { value: T } | null = null;

  return {
    publish(value: T): void {
      if (listeners.size === 0) {
        if (replayLast) pending = { value };
        return;
      }
      for (const listener of Array.from(listeners)) listener(value);
    },
    subscribe(listener: (value: T) => void): () => void {
      // A fresh wrapper per subscription so the same function subscribed
      // twice is two independent subscriptions.
      const entry = (value: T) => listener(value);
      listeners.add(entry);
      if (pending) {
        const { value } = pending;
        pending = null;
        entry(value);
      }
      return () => {
        listeners.delete(entry);
      };
    },
  };
}
