import { useSyncExternalStore } from "react";
import {
  listGroupFormSections,
  listManageViews,
  listServerFormSections,
  listServerSettingsTabs,
  type ServerFormTabId,
  subscribeFormSlots,
} from "./registry";

/** Spec 0038, Abschnitt 4: rendert die registrierten Abschnitte des
 * Server-Formulars. Ohne Registrierung wird nichts gerendert. */
export function ServerFormSections({
  serverId,
  dirty,
  tab = "general",
}: {
  serverId: string;
  dirty: boolean;
  tab?: ServerFormTabId;
}) {
  const all = useSyncExternalStore(subscribeFormSlots, listServerFormSections);
  const sections = all.filter((s) => (s.tab ?? "general") === tab);
  return (
    <>
      {sections.map(({ id, component: Section }) => (
        <div key={id} data-extension-section={id}>
          <Section serverId={serverId} dirty={dirty} />
        </div>
      ))}
    </>
  );
}

/** Sections placed in a tab whose panel is not rendered by the extension
 * layer (Connection/Security of a saved server). Renders nothing, not even a
 * wrapper, when no section targets the tab; otherwise a labelled region that
 * follows the tab's visibility. */
export function ServerFormTabSections({
  serverId,
  dirty,
  tab,
  hidden,
  labelledBy,
}: {
  serverId: string;
  dirty: boolean;
  tab: ServerFormTabId;
  hidden: boolean;
  labelledBy: string;
}) {
  const all = useSyncExternalStore(subscribeFormSlots, listServerFormSections);
  if (!all.some((s) => (s.tab ?? "general") === tab)) return null;
  return (
    <section aria-labelledby={labelledBy} hidden={hidden} className="space-y-6">
      <ServerFormSections serverId={serverId} dirty={dirty} tab={tab} />
    </section>
  );
}

/** Registered server settings tabs (re-renders on later registration). */
export function useServerSettingsTabs() {
  return useSyncExternalStore(subscribeFormSlots, listServerSettingsTabs);
}

export function GroupFormSections({ groupId, dirty }: { groupId: string; dirty: boolean }) {
  const sections = useSyncExternalStore(subscribeFormSlots, listGroupFormSections);
  return (
    <>
      {sections.map(({ id, component: Section }) => (
        <div key={id} data-extension-section={id}>
          <Section groupId={groupId} dirty={dirty} />
        </div>
      ))}
    </>
  );
}

/** Registrierte Verwalten-Ansichten (re-rendert bei späterer Registrierung). */
export function useManageViews() {
  return useSyncExternalStore(subscribeFormSlots, listManageViews);
}
