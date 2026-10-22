// Issue #579, Spec 0113: the top-level navigation entries, shared by the
// wide layout (`MainScreen`) and the narrow burger menu so they cannot drift.
export type MainTab = "connect" | "manage" | "rules";

export const MAIN_TAB_ENTRIES: ReadonlyArray<{ id: MainTab; labelKey: string }> = [
  { id: "connect", labelKey: "nav.connect" },
  { id: "manage", labelKey: "nav.manage" },
  { id: "rules", labelKey: "nav.rules" },
];
