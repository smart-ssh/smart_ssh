import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { EventPayloads } from "./eventPayloads";

/** Subscribes to one backend event; the payload type comes from `EventPayloads`
 * (generated from the Rust payload structs, issue #920), so a payload change
 * is a type error at every listener. */
export function listenTyped<K extends keyof EventPayloads>(
  name: K,
  handler: (payload: EventPayloads[K]) => void,
): Promise<UnlistenFn> {
  return listen<EventPayloads[K]>(name, (e) => handler(e.payload));
}
