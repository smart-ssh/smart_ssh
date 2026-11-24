// ADR 0122, R13/R14: `fireEvent` on a detached or disabled element is a silent
// no-op in the DOM; the failure then shows up later as an unrelated assertion.
// The guard turns it into an immediate, readable error.
import type { fireEvent as FireEvent } from "@testing-library/react";

let allowDisabled = 0;

/** Runs `fn` with the disabled-element check off (used by `clickDisabled`). */
export function withDisabledAllowed<T>(fn: () => T): T {
  allowDisabled += 1;
  try {
    return fn();
  } finally {
    allowDisabled -= 1;
  }
}

function describeNode(node: Node): string {
  if (!(node instanceof Element)) return node.nodeName;
  const parts = [`<${node.tagName.toLowerCase()}>`];
  const role = node.getAttribute("role");
  if (role) parts.push(`role="${role}"`);
  const name =
    node.getAttribute("aria-label") ?? (node.textContent ?? "").trim().slice(0, 40);
  if (name) parts.push(`name="${name}"`);
  return parts.join(" ");
}

export function checkFireEventTarget(event: string, target: unknown): void {
  if (!(target instanceof Node)) return;
  // `window` is not a Node; `document` is allowed.
  if (target.nodeType === Node.DOCUMENT_NODE) return;
  if (!target.isConnected) {
    throw new Error(
      `fireEvent.${event} on a detached element (${describeNode(target)}): it is ` +
        "no longer in the document, so the event does nothing. Query the element " +
        "again after the transition (ADR 0122, R14).",
    );
  }
  if (allowDisabled === 0 && target instanceof Element && target.matches(":disabled")) {
    throw new Error(
      `fireEvent.${event} on a disabled element (${describeNode(target)}): the ` +
        "event does nothing. Wait for the rendered state that enables it " +
        "(ADR 0122, R13), or use clickDisabled() from src/test/clickDisabled to " +
        "prove a disabled control does nothing.",
    );
  }
}

/** Wraps the callable `fireEvent` and every `fireEvent.<event>` method. */
export function guardFireEvent(actual: typeof FireEvent): typeof FireEvent {
  const wrapped = ((target: unknown, event: Event) => {
    checkFireEventTarget(event?.type ?? "event", target);
    return actual(target as Element, event);
  }) as typeof FireEvent;
  for (const key of Object.keys(actual) as (keyof typeof FireEvent)[]) {
    const original = actual[key] as unknown;
    if (typeof original !== "function") continue;
    (wrapped as unknown as Record<string, unknown>)[key] = (
      target: unknown,
      ...rest: unknown[]
    ) => {
      checkFireEventTarget(String(key), target);
      return (original as (...a: unknown[]) => unknown)(target, ...rest);
    };
  }
  return wrapped;
}
