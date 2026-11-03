import { useEffect, useRef } from "react";
import { onConnectionStatusChanged } from "../../events";

/** Connection lost / restored handling of the file browser. */
export function useConnectionLoss(
  sessionId: string,
  updateElevation: (next: null) => void,
  reloadAfterReconnect: () => void,
) {
  const connectionLostRef = useRef(false);

  // Verbindung weg → erhöhter Kanal ist weg (lebt nur in der Session).
  // Issue #355: nach einem Reconnect das bisherige Verzeichnis neu laden
  // (Home, falls es nicht mehr existiert). Der erhöhte Kanal kommt nicht
  // zurück, daher immer im normalen Modus.
  useEffect(() => {
    const unlisten = onConnectionStatusChanged((event) => {
      if (event.sessionId !== sessionId) return;
      if (event.status !== "connected") {
        connectionLostRef.current = true;
        updateElevation(null);
      } else if (connectionLostRef.current) {
        connectionLostRef.current = false;
        reloadAfterReconnect();
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
    // `updateElevation` liest nur Refs/State-Setter und `sessionId`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId]);
}
