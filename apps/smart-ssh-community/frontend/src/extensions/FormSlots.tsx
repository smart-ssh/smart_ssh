import { useSyncExternalStore } from "react";
import {
  listGroupFormSections,
  listManageViews,
  listServerFormSections,
  subscribeFormSlots,
} from "./registry";

/** Spec 0038, Abschnitt 4: rendert die registrierten Abschnitte des
 * Server-Formulars. Ohne Registrierung wird nichts gerendert. */
export function ServerFormSections({ serverId, dirty }: { serverId: string; dirty: boolean }) {
  const sections = useSyncExternalStore(subscribeFormSlots, listServerFormSections);
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
