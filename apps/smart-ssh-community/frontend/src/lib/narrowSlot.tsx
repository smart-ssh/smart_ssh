// Issue #1461: the title-bar slot of the narrow layout (`NarrowHeaderNav`)
// is handed to `SessionView` through React state instead of a DOM query, so
// it does not matter in which commit the slot and the session view mount.
import { createContext, useContext, type ReactNode } from "react";

interface NarrowSlotValue {
  slot: HTMLElement | null;
  setSlot: (el: HTMLElement | null) => void;
}

const NarrowSlotContext = createContext<NarrowSlotValue>({ slot: null, setSlot: () => {} });

export function NarrowSlotProvider({ value, children }: { value: NarrowSlotValue; children: ReactNode }) {
  return <NarrowSlotContext.Provider value={value}>{children}</NarrowSlotContext.Provider>;
}

export function useNarrowSlot(): NarrowSlotValue {
  return useContext(NarrowSlotContext);
}
