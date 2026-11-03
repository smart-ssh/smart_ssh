import type { ReactNode } from "react";
import { ServerFormSections } from "../../extensions/FormSlots";
import { TabPanel } from "../ui";

interface Props {
  idBase: string;
  active: boolean;
  serverId: string;
  dirty: boolean;
  /** The notes editor itself: the shared notes panel of a remote server or
   * the local pseudo-server's own editor. */
  children: ReactNode;
}

/** Notes tab frame shared by the local and the remote form: the panel, the
 * editor and the notes-tab extension sections. */
export function NotesTab({ idBase, active, serverId, dirty, children }: Props) {
  return (
    <TabPanel idBase={idBase} id="notes" active={active} className="space-y-6">
      {children}
      <ServerFormSections serverId={serverId} dirty={dirty} tab="notes" />
    </TabPanel>
  );
}
