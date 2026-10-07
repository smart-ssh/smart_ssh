# ADR 0109 — Eine eigene MCP-Sitzung je Server und MCP-Client

Status: akzeptiert
Betrifft: Spec 0103, Spec 0017 (Abschnitt 3), Spec 0028 (Abschnitt 9a),
Spec 0040 (Abschnitt 4), Spec 0057 (Abschnitt 2.1), Issue #50

## Problem

Eine MCP-Anfrage nahm bisher die offene, verbundene Sitzung des Nutzers für
denselben Server. MCP-Aktionen erschienen im Tab des Nutzers, MCP-Ausgaben
gingen in dessen KI-Verlauf, und der Tab wurde nach vorne geholt. Issue #50
legt das Ziel fest: eigene Verbindung, eigener Tab und eigener Kontext je
(Server, MCP-Client), kein Fokus-Wechsel.

## Entscheidung

1. **Zuordnung über eine eigene Registry statt über eine Markierung an der
   `Session`.** `app_logic::mcp_sessions::McpSessionRegistry` (in
   `AppState.mcp`) bildet (Server, Client) auf die aktuelle Sitzung ab und
   kennt alle offenen MCP-Sitzungen. `ensure_session` fragt nur diese
   Registry. Eine Nutzer-Sitzung steht dort nie und kann deshalb nicht
   getroffen werden. Die Trennung ist damit eine Eigenschaft der Zuordnung,
   kein zusätzlicher Zweig im Ausführungspfad. Filter-Engine, Confirm-Zwang,
   Redaction und Fencing bleiben unverändert und gelten für die MCP-Sitzung
   wie für jede andere `Session`. `Session`/`SessionParts` bleiben unverändert.
2. **Client-Identität = `clientInfo.name`, getrimmt.** Das ist die einzige
   Unterscheidung, die das Protokoll liefert. Sie ist selbst deklariert und
   dient der Trennung der Arbeitsbereiche, nicht der Autorisierung: Alle
   Clients nutzen dasselbe Token und dieselbe Allow-Liste. Clients ohne
   Namen teilen sich je Server eine Sitzung.
3. **Eintragen vor dem Verbindungsaufbau, Anlege-Lock je Schlüssel.** So
   ist der Tab schon während eines Host-Key-Dialogs als MCP-Tab
   gekennzeichnet, und zwei gleichzeitige Anfragen desselben Clients bauen
   nicht zwei Verbindungen auf. Je Schlüssel statt global, damit ein offener
   Dialog für einen Server keine Anfrage an einen anderen aufhält.
4. **Nur-lesende MCP-Ansicht, im Backend durchgesetzt.** Der MCP-Tab zeigt
   Aktionskarten und Ergebnisse, aber kein Terminal, keinen Dateibrowser
   und keine Chat-Eingabe. `send_chat_message`,
   `continue_truncated_response` und `open_terminal` lehnen MCP-Sitzungen
   ab. Ein interaktives Terminal hätte Nutzer-Eingaben auf derselben
   Verbindung bedeutet, auf der der Agent arbeitet, und genau diese
   Vermischung soll Issue #50 beenden.
5. **Schließen lehnt ab (fail closed), im Backend.** `disconnect` trägt eine
   MCP-Sitzung aus und löst ihre wartende Bestätigung mit `Deny` auf, statt
   sich auf den Frontend-Fluss zu verlassen. Der kann nach einem Reload die
   `actionId` nicht mehr kennen, und dann bliebe nur der Timeout. Der Client
   bekommt eine eigene Meldung statt „vom Nutzer abgelehnt“. Für
   Nutzer-Tabs bleibt das Schließen unverändert. Ob das Backend auch dort
   ablehnen soll, ist eine eigene Frage außerhalb von Issue #50.
6. **Abgerissene Verbindung:** Die nächste Anfrage legt eine neue Sitzung
   an. Der alte Tab bleibt bis zum Schließen als MCP-Tab gekennzeichnet.
   Die Sitzung wird nicht unter derselben ID neu verbunden, weil die ID der
   Schlüssel des Tabs und aller Events ist.
7. **`mcp_origin_flags` bleibt.** Persistenz-Ausschluss (Spec 0040) und
   Summary-Ausschluss (Spec 0057) hängen daran. In einer MCP-Sitzung ist
   jeder Eintrag MCP-originiert, die Flags schaden also nicht und sichern
   ab, falls je wieder gemischt würde.

## Konsequenzen

- Ein MCP-Client öffnet je Server eine zusätzliche SSH-Verbindung, auch wenn
  der Nutzer schon verbunden ist.
- Der Nutzer sieht pro Server bis zu einen Nutzer-Tab und je MCP-Client
  einen MCP-Tab.
- Die Chat-KI des Nutzers sieht keine MCP-Ausgaben mehr, auch keine
  hilfreichen. Das ist gewollt, denn MCP-Inhalte sind fremder Kontext.
