import { describe, expect, it } from "vitest";
import { createBus } from "./createBus";

describe("createBus", () => {
  it("delivers in subscription order", () => {
    const bus = createBus<number>();
    const seen: string[] = [];
    bus.subscribe((v) => seen.push(`a${v}`));
    bus.subscribe((v) => seen.push(`b${v}`));
    bus.publish(1);
    expect(seen).toEqual(["a1", "b1"]);
  });

  it("keeps delivering when a listener unsubscribes itself", () => {
    const bus = createBus<number>();
    const seen: string[] = [];
    const off = bus.subscribe(() => {
      seen.push("a");
      off();
    });
    bus.subscribe(() => seen.push("b"));
    bus.publish(1);
    bus.publish(2);
    expect(seen).toEqual(["a", "b", "b"]);
  });

  it("does not deliver the current event to a listener added during delivery", () => {
    const bus = createBus<number>();
    const seen: string[] = [];
    bus.subscribe(() => {
      seen.push("a");
      bus.subscribe(() => seen.push("late"));
    });
    bus.publish(1);
    expect(seen).toEqual(["a"]);
  });

  it("replays the pending value exactly once to the first subscriber", () => {
    const bus = createBus<number>({ replayLast: true });
    bus.publish(1);
    bus.publish(2);
    const first: number[] = [];
    const second: number[] = [];
    bus.subscribe((v) => first.push(v));
    bus.subscribe((v) => second.push(v));
    expect(first).toEqual([2]);
    expect(second).toEqual([]);
    bus.publish(3);
    expect(first).toEqual([2, 3]);
    expect(second).toEqual([3]);
  });

  it("does not replay without the option", () => {
    const bus = createBus<number>();
    bus.publish(1);
    const seen: number[] = [];
    bus.subscribe((v) => seen.push(v));
    expect(seen).toEqual([]);
  });

  it("unsubscribe is idempotent", () => {
    const bus = createBus<number>();
    const seen: number[] = [];
    const off = bus.subscribe((v) => seen.push(v));
    const other: number[] = [];
    bus.subscribe((v) => other.push(v));
    off();
    off();
    bus.publish(1);
    expect(seen).toEqual([]);
    expect(other).toEqual([1]);
  });
});
