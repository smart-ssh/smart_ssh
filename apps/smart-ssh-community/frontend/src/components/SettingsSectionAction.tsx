import { createContext, useContext, type ReactNode } from "react";
import { createPortal } from "react-dom";

/** The action slot in the settings section header (Issue #1376); `null` outside the settings screen. */
export const SettingsActionSlotContext = createContext<HTMLElement | null>(null);

/**
 * A settings section renders this to put an action (e.g. "+ Add provider")
 * into the right side of its section header. Renders nothing outside the
 * settings screen or before the slot is mounted.
 */
export function SettingsSectionAction({ children }: { children: ReactNode }) {
  const slot = useContext(SettingsActionSlotContext);
  return slot ? createPortal(children, slot) : null;
}
