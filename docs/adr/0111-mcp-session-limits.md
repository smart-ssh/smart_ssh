# ADR 0111 — Grenzen für MCP-Client-Namen und MCP-Sitzungen je Server

Status: akzeptiert
Betrifft: Spec 0104 (Abschnitt 2), ADR 0109, Issue #68

## Problem

Seit Spec 0104 bekommt jeder MCP-Client je Server eine eigene Sitzung mit
eigener SSH-Verbindung und eigenem Tab. Der Client wird über den frei
wählbaren `clientInfo.name` unterschieden. Ein Client mit dem MCP-Token
konnte damit über beliebig viele verschiedene Namen beliebig viele
Verbindungen und Hintergrund-Tabs je erlaubtem Server öffnen, beliebig
lange Namen in Tab-Beschriftung und OS-Benachrichtigung schreiben, und die
Tabelle der Anlege-Locks wuchs mit jedem je gesehenen Namen. Ein Umgehen
der Bestätigung ist das nicht (jede MCP-Aktion bleibt `Confirm`), aber
Ressourcenverbrauch und UI-Unordnung ohne Grenze.

Issue #68 lässt zwei Werte offen und empfiehlt je einen.

## Entscheidung

1. **Client-Name höchstens 64 Zeichen** (`MCP_CLIENT_NAME_MAX_CHARS`),
   gezählt in Unicode-Zeichen, nicht Bytes, Schnitt an einer
   Zeichengrenze. Das reicht für reale Client-Namen und hält Tab und
   Benachrichtigung lesbar. Der gekürzte Name gilt überall (Schlüssel,
   Tab, Benachrichtigung, Bestätigungsdialog), damit Anzeige und Zuordnung
   nie auseinanderlaufen.
2. **Höchstens 4 offene MCP-Sitzungen je Server, fest**
   (`MCP_MAX_SESSIONS_PER_SERVER`). Keine Einstellung, solange kein Bedarf
   belegt ist. Es zählt jede eingetragene Sitzung, auch eine abgerissene
   mit noch offenem Tab: Der Tab belegt weiter Platz in der UI, und so
   kann ein Client die Grenze nicht durch wiederholtes Abreißen umgehen.
   Die Prüfung sitzt in `McpSessionRegistry::register` unter dem
   Registry-Lock, nicht unter dem Anlege-Lock je Schlüssel. Sonst könnten
   gleichzeitige Anfragen verschiedener Clients die Grenze gemeinsam
   überschreiten.
3. **Fehler an den Client als `ActionOutcome::Failed`**, wie bei einer in
   der App geschlossenen Sitzung. Die Schnittstelle `McpBackend` /
   `LookupError` bleibt unverändert, und der Client bekommt eine lesbare
   Meldung statt "unbekannter Server".
4. **Anlege-Lock mit Nutzerzähler statt `Arc::strong_count`.** Jeder
   Aufrufer von `lock_creation` meldet sich unter dem Map-Lock an und
   beim Fallen seines Guards wieder ab, auch wenn das Warten abgebrochen
   wird. Bei 0 wird der Eintrag entfernt. Der Guard gibt erst das Lock
   frei und meldet sich danach ab. So kann für einen Schlüssel nie ein
   zweites Lock entstehen, solange jemand das erste hält oder darauf
   wartet. Ein später Ankommender bekommt ein neues Lock, findet die unter
   dem alten Lock eingetragene Sitzung aber über `reusable_session`.
5. **`acquire_session` in `app_logic`.** "Wiederverwenden oder eintragen
   und Tab ankündigen" ist von `app_shell::mcp_backend::ensure_session` in
   die Tauri-freie Registry gewandert (Spec 0084). Damit ist testbar, dass
   bei erreichter Grenze weder ein Eintrag noch ein
   `mcp-action-tab-requested`-Event entsteht. Ein Verbindungsaufbau folgt
   nur auf `McpSessionSlot::New`.

## Abgewogene Alternativen

- **32 oder 128 Zeichen:** 32 schneidet reale, beschreibende Namen ab, 128
  sprengt Tab-Beschriftung und Benachrichtigung.
- **Einstellbare Höchstzahl:** mehr UI und Persistenz ohne belegten Bedarf.
  Lässt sich später ergänzen, ohne diese Entscheidung zu brechen.
- **Nur eine globale Höchstzahl:** ein Client könnte alle Plätze auf einem
  Server belegen. Je Server ist die Einheit, die der Nutzer in der Tab-Leiste
  sieht.
- **Lock-Eintrag in `unregister` entfernen:** Ein Eintrag entsteht auch
  für Anfragen, die nie eine Sitzung eintragen (Grenze erreicht,
  Verbindungsaufbau gescheitert, Anfrage abgebrochen). Aufräumen beim
  Fallen des Guards deckt alle Fälle ab.

## Konsequenzen

- Ein fünfter MCP-Client auf demselben Server bekommt eine Fehlermeldung,
  bis der Nutzer einen MCP-Tab dieses Servers schließt.
- Clients, deren Namen sich erst nach Zeichen 64 unterscheiden, teilen sich
  eine Sitzung.
