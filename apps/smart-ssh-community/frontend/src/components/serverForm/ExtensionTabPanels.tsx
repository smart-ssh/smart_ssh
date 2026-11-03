import type { useServerSettingsTabs } from "../../extensions/FormSlots";
import { ErrorBoundary, TabPanel } from "../ui";

interface Props {
  idBase: string;
  serverId: string;
  tabs: ReturnType<typeof useServerSettingsTabs>;
  /** Ids (`ext:<id>`) of the tabs that were opened so far. */
  mounted: ReadonlySet<string>;
  activeTab: string;
}

/** Extension tabs (Issue #608), lazily mounted when first opened and kept
 * mounted while the server stays selected. */
export function ExtensionTabPanels({ idBase, serverId, tabs, mounted, activeTab }: Props) {
  return (
    <>
      {tabs
        .filter((x) => mounted.has(`ext:${x.id}`))
        .map((x) => (
          <TabPanel
            key={x.id}
            idBase={idBase}
            id={`ext:${x.id}`}
            active={activeTab === `ext:${x.id}`}
            className="space-y-6"
          >
            <ErrorBoundary id={`server-settings-tab:${x.id}`} resetKeys={[serverId]}>
              <x.component serverId={serverId} />
            </ErrorBoundary>
          </TabPanel>
        ))}
    </>
  );
}
