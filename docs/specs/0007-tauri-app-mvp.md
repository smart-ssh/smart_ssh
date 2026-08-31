# Spec: Tauri-App — Architektur & MVP-Screens

Status: Entwurf
Modul: `crates/app-tauri` (Backend-Wrapper) + `frontend/` (Web-UI)
Abhängigkeiten: alle bisherigen Core-Module (`ssh-manager-core`,
`persistence-sqlite`, `ssh-transport`, `ai-providers`) — dieses Modul
verdrahtet sie erstmals zu einer benutzbaren App.

## 1. Ziel

Dieses Modul ist bewusst in zwei Ausbaustufen unterteilt:

- **MVP-Slice 1**: der kleinstmögliche Screen, der den kompletten
  Kern-Workflow einmal echt erlebbar macht — Server auswählen, verbinden,
  Terminal + Chat nebeneinander, KI schlägt Kommando vor, Nutzer bestätigt,
  Ausführung passiert, Ergebnis geht zurück an die KI. Enthält bewusst schon
  eine einfache **AI-Provider-Verwaltung** (Abschnitt 8), da ohne
  hinterlegten API-Key kein echter Test möglich ist — aber weiterhin keine
  Server-/Gruppen-Anlege-UI.
- **Ausbaustufe 2** (eigene Folge-Spec): Server-/Gruppen-Verwaltung im UI,
  Notiz-Historie, mehrere Tabs/Sessions parallel.

Diese Spec deckt nur die Architektur (die für beide Stufen trägt) und den
MVP-Slice-1-Screen ab.

## 2. Frontend-Stack-Entscheidung

**React + TypeScript**, Styling über Tailwind, Terminal über `xterm.js`.
Begründung: größtes Tauri-Ökosystem/meiste Referenzbeispiele, gute
Verfügbarkeit fertiger Komponenten für Chat-UIs und Split-Panes. Das ist eine
pragmatische Default-Entscheidung, kein hartes technisches Erfordernis — falls
du eine andere Präferenz hast (Svelte/Solid), lässt sich das an dieser Stelle
noch ohne größere Folgekosten ändern, da noch kein Frontend-Code existiert.

## 3. Architektur: `app-tauri` als dünner Wrapper

`app-tauri` enthält **keine fachliche Logik**, nur:
- Tauri-Commands, die Core-APIs aufrufen und Ergebnisse als DTOs (serde-
  serialisierbare Structs) an das Frontend zurückgeben
- Tauri-Events, um asynchrone Streams (Chat-Tokens, Terminal-Output) an das
  Frontend zu pushen
- In-Memory-`AppState` (Tauri-managed state), das laufende Sessions hält

```rust
pub struct AppState {
    pub sessions: Mutex<HashMap<SessionId, Session>>,
    pub profile_store: Arc<dyn ProfileStore>,      // Spec 0003/0004
    pub credential_store: Arc<dyn CredentialStore>, // Spec 0003
    pub host_key_store: Arc<dyn HostKeyStore>,      // Spec 0005
}

pub struct Session {
    pub transport: Box<dyn SshTransport>,   // Spec 0005
    pub ai_provider: Box<dyn AiProvider>,   // Spec 0006
    pub context: SessionContext,             // wächst mit jeder Chat-Runde
    pub filter_engine: Arc<FilterEngine<...>>, // Spec 0002
}
```

## 4. Tauri-Commands (MVP-Slice 1)

```
list_servers() -> Vec<ServerDto>
connect(server_id) -> SessionId
    // kann während des Aufbaus ein host-key-verification-needed Event auslösen
confirm_host_key(session_id, decision: Trust | Reject)
open_terminal(session_id) -> ()
    // startet PTY-Channel, Output kommt fortan über terminal-output Events
terminal_input(session_id, data: Vec<u8>)
terminal_resize(session_id, cols: u16, rows: u16)
send_chat_message(session_id, text: String)
respond_to_action(session_id, action_id, decision: Approve | Deny | EditThenApprove { command: String })
disconnect(session_id)

list_ai_providers() -> Vec<AiProviderConfigDto>
add_ai_provider(config: AiProviderConfigInput) -> ProviderId
update_ai_provider(id: ProviderId, config: AiProviderConfigInput)
delete_ai_provider(id: ProviderId)
set_active_ai_provider(id: ProviderId)
```

## 5. Tauri-Events (Push Richtung Frontend)

```
connection-status-changed   { session_id, status }
host-key-verification-needed { session_id, host, port, kind: Unknown | Mismatch, fingerprint }
terminal-output              { session_id, data: Vec<u8> }
chat-text-delta               { session_id, delta: String }
chat-action-proposed          { session_id, action_id, action: AiAction, decision: AutoExec | Confirm | Deny }
chat-action-result            { session_id, action_id, output: CommandOutput }
```

Wichtig für die Transparenz-Philosophie des Projekts: **Auch ein `Deny`
durch die Filter-Engine wird als `chat-action-proposed`-Event mit
`decision: Deny` an das Frontend geschickt, nicht stillschweigend verworfen.**
Der Nutzer soll sehen können, dass die KI etwas vorgeschlagen hat, das
blockiert wurde — inklusive des Grundes aus `Decision::Deny { reason }`
(Spec 0002). Bei `AutoExec` wird das Event ebenfalls geschickt (informativ),
aber ohne auf `respond_to_action` zu warten — die Ausführung läuft parallel
sofort an.

## 6. Ablauf eines Kommando-Vorschlags (Kernschleife)

1. Nutzer schreibt eine Chat-Nachricht → `send_chat_message`
2. Backend baut `SessionContext` (inkl. `effective_notes()`, Spec 0003) und
   ruft `AiProvider::send()` auf
3. `AiEvent::TextDelta` → sofort als `chat-text-delta` weitergereicht
4. `AiEvent::ActionProposed(AiAction::SuggestCommand { command })` → Backend
   ruft `FilterEngine::evaluate(command, ctx)` auf (Spec 0002)
5. Ergebnis wird als `chat-action-proposed` gesendet:
   - `AutoExec` → Backend führt sofort über `SshTransport::execute()` aus
     (Spec 0005), Output läuft durch `OutputRedactor` (Spec 0006), Ergebnis
     als `chat-action-result` Event **und** zusätzlich als
     `MessageContent::CommandResult` in den `SessionContext` für die nächste
     KI-Runde übernommen
   - `Confirm` → Backend wartet auf `respond_to_action` vom Frontend, bevor
     etwas ausgeführt wird
   - `Deny` → keine Ausführung möglich, Event informiert nur
6. Bei `AiAction::ProposeNoteUpdate` (Spec 0003, Abschnitt 5.2): analoges
   Event, aber **immer** wartend auf Bestätigung, nie `AutoExec` — das gilt
   unabhängig von der Filter-Engine, wie in Spec 0003 festgelegt
7. **Automatische Folgerunde:** wurde in der aktuellen Runde tatsächlich
   eine Aktion ausgeführt (`AutoExec` oder vom Nutzer bestätigt — nicht bei
   `Deny`), ruft das Backend `AiProvider::send()` unmittelbar erneut auf,
   mit dem inzwischen um das `CommandResult`/die Notiz-Zusammenfassung
   erweiterten `SessionContext` (Punkt 5), statt auf eine neue
   `send_chat_message`-Nachricht vom Nutzer zu warten. Ohne diesen
   Automatismus bekäme der Nutzer nach einem ausgeführten Kommando nur den
   rohen Output zu sehen, nie eine tatsächliche Antwort der KI dazu. Jede in
   einer Folgerunde neu vorgeschlagene Aktion durchläuft erneut dieselbe
   Filter-Engine/Bestätigungslogik wie jede andere (Punkt 4/5) — dieser
   Automatismus betrifft nur den Rückruf an die KI, nicht die
   Bestätigungspflicht einzelner Aktionen. Begrenzt auf eine feste maximale
   Rundenzahl pro Nutzer-Nachricht, damit eine KI, die immer wieder neue
   Aktionen vorschlägt, nicht unbegrenzt weiterläuft — wird die Grenze
   erreicht, bricht das Backend mit einer Fehlermeldung ab, statt weiter zu
   warten.

## 7. MVP-Slice-1-Screen (UI-Umfang)

Bewusst minimal, um schnell zum ersten echten End-to-End-Test zu kommen:

- Einfache Serverliste (aus bereits bestehender SQLite-Persistenz, **keine**
  Anlege-/Bearbeiten-UI für Server/Gruppen in diesem Schritt — Testserver
  werden vorerst direkt über das `profiles_demo`-Beispiel oder einen kleinen
  CLI-Helfer angelegt)
- Klick auf Server → Verbindung, bei `Unknown`/`Mismatch` Host-Key-Dialog
  (Abschnitt 6, Spec 0005)
- Ein Screen: Terminal links (xterm.js, verbunden über `terminal-*`
  Commands/Events), Chat-Panel rechts
- Bestätigungs-Dialog für `Confirm`-Aktionen: zeigt das exakte Kommando,
  erlaubt Editieren vor Bestätigung (deckt `EditThenApprove` ab), zwei klare
  Buttons (Ausführen/Ablehnen)
- Ein einfacher Settings-Screen/Modal für die AI-Provider-Verwaltung
  (Abschnitt 8) — im Gegensatz zu Server-/Gruppen-Verwaltung **ist** das
  bereits Teil von Slice 1, da ohne funktionierenden Provider kein
  End-to-End-Test möglich ist
- Ladeanzeige im Chat-Panel: solange auf eine Reaktion der KI gewartet wird
  (zwischen `send_chat_message`-Aufruf und dem ersten `chat-text-delta`
  dieser Runde, ebenso in jeder Wartepause danach — z. B. während der
  Nutzer eine `Confirm`-Aktion noch nicht bestätigt hat oder nach deren
  Ausführung auf die nächste KI-Antwort gewartet wird), zeigt das Chat-Panel
  einen Lade-Indikator anstelle einer stummen, nicht unterscheidbaren
  Wartezeit. Kein separates Event nötig: der Zustand ergibt sich rein
  clientseitig daraus, ob die `send_chat_message`-Anfrage noch aussteht
  *und* der zuletzt angezeigte Chat-Eintrag keine bereits laufende
  Assistenten-Antwort ist.

## 8. AI-Provider-Verwaltung

Persistiert die nicht-geheimen Teile einer Provider-Konfiguration in der
SQLite-DB (Erweiterung von Spec 0004 um eine neue Tabelle), den API-Key
selbst ausschließlich im `CredentialStore` (Spec 0003) — nie in der DB.

### 8.1 Schema-Erweiterung (`persistence-sqlite`)

```sql
-- migrations/0002_ai_provider_configs.sql

CREATE TABLE ai_provider_configs (
    id                          TEXT PRIMARY KEY,
    provider_type               TEXT NOT NULL CHECK (
        provider_type IN ('openai', 'anthropic', 'generic_openai_compatible', 'ollama')
    ),
    display_name                TEXT NOT NULL,
    base_url                    TEXT,             -- nur für generic/ollama relevant
    model                       TEXT NOT NULL,
    supports_native_tool_calling BOOLEAN NOT NULL DEFAULT TRUE,
    credential_ref              TEXT NOT NULL,    -- Schlüssel in den CredentialStore
    is_active                   BOOLEAN NOT NULL DEFAULT FALSE,
    created_at                  TEXT NOT NULL,
    updated_at                  TEXT NOT NULL
);

-- höchstens ein aktiver Provider gleichzeitig (MVP-Annahme, s. Spec 0006 Abschnitt 8)
CREATE UNIQUE INDEX idx_ai_provider_single_active
    ON ai_provider_configs(is_active) WHERE is_active = TRUE;
```

### 8.2 DTOs und Verhalten

```rust
pub struct AiProviderConfigInput {
    pub provider_type: ProviderType,
    pub display_name: String,
    pub base_url: Option<String>,
    pub model: String,
    pub supports_native_tool_calling: bool,
    pub api_key: String, // niemals persistiert, nur zur Weitergabe an CredentialStore
}

pub struct AiProviderConfigDto {
    pub id: ProviderId,
    pub provider_type: ProviderType,
    pub display_name: String,
    pub base_url: Option<String>,
    pub model: String,
    pub supports_native_tool_calling: bool,
    pub is_active: bool,
    // bewusst KEIN api_key-Feld — der Key geht nie zurück ans Frontend
}
```

Ablauf bei `add_ai_provider`: Backend generiert eine neue `ProviderId`,
erzeugt daraus einen `CredentialRef` (z. B. `"ai-provider:{id}"`), speichert
den `api_key` über `CredentialStore::set()`, und erst danach die restlichen
(nicht-geheimen) Felder in `ai_provider_configs`. Bei `delete_ai_provider`
symmetrisch: zuerst `CredentialStore::delete()`, dann die DB-Zeile entfernen.
Wird `update_ai_provider` mit leerem `api_key`-Feld aufgerufen, bleibt der
bestehende Credential unverändert (Frontend zeigt den Key ohnehin nie an,
ein leeres Feld heißt "nicht ändern", nicht "auf leer setzen").

### 8.3 UI (Teil von Slice 1)

Einfaches Modal/Screen: Liste konfigurierter Provider (Name, Typ, Modell,
aktiv/inaktiv), Formular zum Hinzufügen (Typ-Dropdown, Name, Modell,
Base-URL-Feld nur bei `generic_openai_compatible`/`ollama` sichtbar,
API-Key-Eingabefeld als Passwortfeld), Lösch-Button pro Eintrag, Auswahl des
aktiven Providers (z. B. Radio-Button/Stern-Icon). Ist noch kein Provider
konfiguriert, wird das Chat-Panel im MVP-Screen durch einen Hinweis ersetzt,
der direkt zu diesem Formular führt, statt eine Chat-Eingabe anzuzeigen, die
ohnehin fehlschlagen würde.

## 9. Offene Punkte

- Reconnect-Verhalten bei Verbindungsabbruch während einer offenen Session
  (siehe bereits offener Punkt aus Spec 0005) muss spätestens hier UI-seitig
  sichtbar werden (z. B. Terminal zeigt "Verbindung getrennt"-Zustand) —
  Umsetzung im Detail noch offen.
- Soll das Löschen eines Providers, der gerade `is_active` ist, verboten
  sein (erst einen anderen aktiv setzen), oder automatisch "kein aktiver
  Provider" zur Folge haben? Tendenz: verbieten, mit klarer Fehlermeldung —
  vermeidet einen impliziten "kein Provider aktiv"-Zustand, den das Frontend
  gesondert behandeln müsste.
