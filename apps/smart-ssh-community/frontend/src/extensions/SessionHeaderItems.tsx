import { useSyncExternalStore } from "react";
import { useChatSessionId } from "../chatSessionIds";
import { listSessionHeaderItems, subscribeFormSlots } from "./registry";

/** Issue #611 (Spec 0038, Abschnitt 4): renders the registered session
 * header items. Renders nothing without registrations. */
export function SessionHeaderItems({
  runtimeSessionId,
  serverId,
}: {
  runtimeSessionId: string;
  serverId: string;
}) {
  const items = useSyncExternalStore(subscribeFormSlots, listSessionHeaderItems);
  const chatSessionId = useChatSessionId(runtimeSessionId);
  if (items.length === 0) return null;
  return (
    <>
      {items.map(({ id, component: Item }) => (
        <div key={id} data-extension-session-header-item={id} className="shrink-0">
          <Item
            runtimeSessionId={runtimeSessionId}
            serverId={serverId}
            chatSessionId={chatSessionId}
          />
        </div>
      ))}
    </>
  );
}
