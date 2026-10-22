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
