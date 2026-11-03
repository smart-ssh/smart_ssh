import { useEffect, useState, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { useServerSettingsTabs } from "../../extensions/FormSlots";
import { resolveSectionLabel } from "../../resolveSectionLabel";
import { useTabsIdBase, type TabItem } from "../ui";
import type { ServerDto } from "../../types";
import type { ServerTab } from "./formTypes";

interface Options {
  serverId: string | null;
  isLocal: boolean;
  loaded: ServerDto | null;
  autoFocusNotes: boolean;
  /** The remote form element, used to find invalid fields per tab. */
  formRef: RefObject<HTMLFormElement | null>;
}

interface TabState {
  /** The server this state belongs to; another server starts fresh. */
  serverId: string | null;
  requestedTab: string;
  invalidTabs: ReadonlySet<ServerTab>;
  openedExtTabs: ReadonlySet<string>;
}

const freshState = (serverId: string | null, requestedTab = "general"): TabState => ({
  serverId,
  requestedTab,
  invalidTabs: new Set(),
  openedExtTabs: new Set(),
});

/** Tab state of the server form (Issue #606, #608): the requested tab, the
 * tabs whose fields fail validation and the lazily mounted extension tabs.
 * Selecting another server starts on General again; this is derived from the
 * `serverId` stored with the state, so no state is set during render. */
export function useServerFormTabs({ serverId, isLocal, loaded, autoFocusNotes, formRef }: Options) {
  const { t, i18n } = useTranslation();
  const isCreate = serverId === null;
  const tabsIdBase = useTabsIdBase();
  const extensionTabs = useServerSettingsTabs();
  const [stored, setStored] = useState<TabState>(() =>
    freshState(serverId, autoFocusNotes && serverId !== null ? "notes" : "general"),
  );
  const [reportValidityPending, setReportValidityPending] = useState(false);
  const state = stored.serverId === serverId ? stored : freshState(serverId);
  const update = (patch: Partial<TabState>) =>
    setStored((prev) => ({ ...(prev.serverId === serverId ? prev : freshState(serverId)), ...patch }));

  const availableTabs: ServerTab[] = isLocal
    ? ["general", "notes"]
    : isCreate
      ? ["general", "connection", "security"]
      : ["general", "connection", "security", "notes"];
  const shownExtTabs = !isCreate && loaded ? extensionTabs : [];
  const tabItems: TabItem[] = [
    ...availableTabs.map((id) => ({
      id: id as string,
      label: id === "notes" ? t("common.notes") : t(`serverForm.tabs.${id}`),
      invalid: state.invalidTabs.has(id),
      invalidLabel: t("serverForm.tabs.invalid"),
    })),
    ...shownExtTabs.map((x) => ({
      id: `ext:${x.id}`,
      label: resolveSectionLabel(x.label, x.id, i18n, t),
    })),
  ];
  // An unregistered active extension tab falls back to General.
  const activeTab = tabItems.some((item) => item.id === state.requestedTab) ? state.requestedTab : "general";

  // An extension tab is mounted when first opened and stays mounted while the
  // server stays selected: the active one counts as opened right away, the
  // effect remembers it for after the user switches away.
  const activeExt = activeTab.startsWith("ext:") ? activeTab : null;
  const mountedExtTabs: ReadonlySet<string> =
    activeExt && !state.openedExtTabs.has(activeExt) ? new Set(state.openedExtTabs).add(activeExt) : state.openedExtTabs;
  useEffect(() => {
    if (activeExt) update({ openedExtTabs: mountedExtTabs });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeExt, serverId]);

  // Tabs whose fields fail the browser validation (the form is `noValidate`,
  // so that an invalid field in a hidden panel cannot silently block saving).
  const collectInvalidTabs = (): Set<ServerTab> => {
    const found = new Set<ServerTab>();
    const panels = formRef.current?.querySelectorAll<HTMLElement>('[role="tabpanel"]') ?? [];
    for (const panel of panels) {
      const tab = (["general", "connection", "security"] as const).find((id) =>
        panel.id.endsWith(`-panel-${id}`),
      );
      if (!tab) continue;
      for (const el of panel.querySelectorAll<HTMLInputElement>("input, select, textarea")) {
        if (!el.checkValidity()) {
          found.add(tab);
          break;
        }
      }
    }
    return found;
  };
  useEffect(() => {
    if (!reportValidityPending) return;
    setReportValidityPending(false);
    formRef.current?.reportValidity();
  }, [reportValidityPending, activeTab, formRef]);

  return {
    tabsIdBase,
    activeTab,
    availableTabs,
    tabItems,
    shownExtTabs,
    mountedExtTabs,
    invalidTabs: state.invalidTabs,
    setActiveTab: (id: string) => update({ requestedTab: id }),
    setInvalidTabs: (invalid: ReadonlySet<ServerTab>) => update({ invalidTabs: invalid }),
    collectInvalidTabs,
    requestReportValidity: () => setReportValidityPending(true),
  };
}

export type ServerFormTabs = ReturnType<typeof useServerFormTabs>;
