# 0050 — Sitzungsende-Notiz-Kürzungs-Dialog (Spec 0057, Etappe 4): Zusammenspiel, Mechanismus, Schwellwert

## Status

Angenommen

## Kontext

Spec 0057 §4.2 verlangt einen Vorschlag am Sitzungsende, eine große
gespeicherte Notiz dauerhaft zu kürzen — mit verpflichtender
Diff-Bestätigung, niemals stillem Überschreiben. Die Spec lässt mehrere
Umsetzungsdetails offen, die während der Implementierung entschieden werden
mussten: das Verhältnis zu `suggest_note_update_on_disconnect` (Etappe 3),
der Schwellwert für "groß", und der technische Mechanismus für einen
KI-Aufruf, der potenziell lange NACH dem eigentlichen `disconnect()`
stattfindet.

## Entscheidungen

### 1. Zusammenspiel mit `suggest_note_update_on_disconnect`: klare Priorität statt kombiniertem Aufruf

Beide Vorschläge laufen im selben `disconnect()`-Hintergrund-Task. Eine
kombinierte KI-Anfrage ("aktualisiere UND kürze in einem Aufruf") wurde
verworfen: §4.2 verlangt explizit, dass der Zusammenfassungs-Aufruf NUR nach
explizitem "Ja, zusammenfassen" läuft, nie automatisch — eine Verschmelzung
mit dem automatischen Update-Vorschlag hätte das verletzt.

Stattdessen: `suggest_note_update_on_disconnect` liefert jetzt `bool`
zurück (`true` ⟺ ein Vorschlag wurde tatsächlich emittiert, unabhängig vom
späteren Annehmen/Ablehnen). `commands::disconnect` ruft
`suggest_note_shrink_on_disconnect` nur auf, wenn dieser Rückgabewert
`false` ist (`orchestration::should_suggest_note_shrink`, eine reine,
direkt getestete Funktion). Der Update-Vorschlag geht vor — er fängt
frisches Sitzungswissen ein, das sonst verloren ginge; der
Kürzungs-Vorschlag ist rein evergreen (eine große Notiz bleibt groß, bis
sie gekürzt wird). Bleibt die Notiz nach einem angenommenen
Update-Vorschlag weiterhin groß, erscheint der Kürzungs-Vorschlag beim
NÄCHSTEN Verbindungsende — nichts geht dauerhaft verloren, es ist reine
zeitliche Entflechtung. Ergebnis: **nie mehr als ein Notiz-Dialog pro
Verbindungsende.**

### 2. Schwellwert: `LARGE_NOTE_DIALOG_THRESHOLD_BYTES = 8_000`

Deutlich über `compaction::MIN_LAST_NOTE_SECTION_BYTES` (2_000 — die
Kompaktierungs-UNTERGRENZE für die *gesendete* Fassung beim verlustfreien
Kürzen, Spec 0057 §4.1, kein "ist groß"-Indikator) und in derselben
Größenordnung wie die spätere Zusammenfassungs-Obergrenze
`NOTE_SHRINK_MAX_BYTES` (4_000): eine Notiz, die schon doppelt so groß ist
wie das, was eine gekürzte Fassung maximal fassen darf, ist ein sinnvoller
Auslöser, ohne bei normal genutzten Notizen (typischerweise wenige hundert
Byte) zu nerven (§4.2, wörtlich: "Nur bei großer Notiz — bei normalen
Notizen kein Dialog").

### 3. Der KI-Aufruf ist bewusst session-unabhängig

Zwischen dem ersten Dialog ("Notiz ist groß …") und dem tatsächlichen Klick
auf "Ja, zusammenfassen" kann beliebig viel Zeit vergehen. Die auslösende
`Session` aus `commands::disconnect`s Hintergrund-Task ist zu diesem
späteren Zeitpunkt typischerweise längst beendet und gedroppt (`disconnect
()` entfernt die Session aus `AppState.sessions`, der besitzende Task läuft
nur so lange wie sein eigener `await`-Ablauf). Der bestehende
`suggest_note_update_on_disconnect`/`execute_note_update`-Pfad ist deshalb
für den zweiten Schritt strukturell nicht wiederverwendbar.

Lösung: `commands::request_note_shrink` (der neue Tauri-Befehl hinter "Ja")
baut einen FRISCHEN `AiProvider` aus der aktuell aktiven
Provider-Konfiguration — derselbe Aufbau-Pfad wie `commands::connect`/
`test_ai_provider_credentials` — und übergibt ihn an die neue, komplett
session-freie Funktion `orchestration::execute_note_shrink_request`. Diese
nimmt `&dyn AiProvider`/`&dyn OutputRedactor`/`server_id: ServerId` direkt
entgegen statt `session: &Session`. `resolve_note_target` wurde dafür von
`session: &Session` auf `server_id: ServerId` umgestellt (der einzige Wert,
den es je aus einer `Session` gelesen hatte) — reines
Signatur-Downcasting, keine Verhaltensänderung für die bestehenden zwei
Aufrufer. Der reine DB-Schreibpfad wurde aus `execute_note_update` in eine
neue, ebenfalls session-unabhängige Funktion `persist_note_revision`
herausgelöst, die beide Aufrufer teilen; `execute_note_update` behält seine
session-abhängigen Nebenwirkungen (Chat-Verlauf-Eintrag, `chat-action-
result`-Event) um diesen gemeinsamen Kern herum.

### 4. Wiederverwendung des bestehenden Diff-Bestätigungsablaufs

`execute_note_shrink_request` emittiert bei einem erfolgreichen KI-Aufruf
**exakt denselben** `note-update-suggested`/`NoteSuggestionToast`/
`NoteDiffPreview`/`ConfirmationRegistry`-Ablauf wie ein regulärer
KI-Notiz-Vorschlag (Spec 0003/0023) — keine zweite, parallele Diff-UI für
dieselbe Sache. `session_id` im Event ist dabei ein frischer,
bedeutungsloser Platzhalter (`Uuid::new_v4()`): es gibt keine lebende
Session, auf die sich der Ablauf bezieht, und `commands::respond_to_action`
ignoriert `session_id` bereits explizit (bestehender Kommentar dort, aus
Spec 0010) — das Feld existiert nur, weil das wiederverwendete
Event-Schema es verlangt. Ein neuer Event-/Payload-Typ nur für diesen einen
Unterschied hätte den Wiederverwendungs-Vorteil zunichtegemacht.

Zwei NEUE, eigene Events decken dagegen die Teile ab, für die es noch keine
passende Wiederverwendung gab: `note-shrink-suggested` (der ERSTE Dialog,
bevor überhaupt ein KI-Aufruf stattfand — `note-update-suggested`s
`action`-Feld verlangt eine bereits fertige `AiAction::ProposeNoteUpdate`,
die an dieser Stelle noch nicht existiert) und `note-shrink-failed` (§4.2/
§6: "KI-Aufruf schlägt fehl → Fehlermeldung" — kein `chat-error`, das an
eine inzwischen womöglich beendete Session gebunden wäre). Beide bewusst
OHNE `session_id`, `server_id` ist der eigentliche Korrelationsschlüssel
(§4.2 bezieht sich immer auf den Server, nie auf eine Sitzung).

### 5. "Mache ich selbst": neuer, minimaler Navigations-Bus statt Prop-Drilling

Der Kürzungs-Vorschlags-Dialog (`NoteShrinkSuggestionToast`) ist wie
`NoteSuggestionToast` app-weit an der `App`-Wurzel gemountet (überlebt
Navigation weg vom Session-Screen, dieselbe Begründung wie Spec 0010
Abschnitt 2 Punkt 6) — außerhalb von `ManagementView`s eigenem,
lokal gehaltenen `selection`-Zustand. "Mache ich selbst" muss trotzdem zur
Bearbeitung eines BESTIMMTEN Servers springen können, ohne die Auswahl quer
durch den Komponentenbaum als Props zu reichen.

Gelöst mit demselben Bus-Muster wie das bereits bestehende
`extensions/featureLockedBus.ts` (Publish/Subscribe außerhalb des
React-Baums): neues `navigationBus.ts` mit `publishRequestServerNoteEdit`/
`subscribeRequestServerNoteEdit`. `App.tsx` abonniert den Bus, schaltet bei
einem Ereignis den aktiven Session-Tab ab (`switchTo(null)`), wechselt zum
"Verwalten"-Tab und reicht die gewünschte `Selection` als neuen,
optionalen `initialSelection`-Prop an `ManagementView` durch (dort per
Effekt übernommen, da sich der Wert nach dem Mounten noch ändern kann,
anders als ein normaler `useState`-Startwert).

## Konsequenzen

- Genau ein zusätzlicher KI-Aufruf, ausschließlich nach explizitem "Ja,
  zusammenfassen" — kein automatischer Zusammenfassungsversuch.
- `commands::request_note_shrink` liest den API-Key synchron aus dem
  `CredentialStore` (wie jeder andere `build_ai_provider`-Aufruf, Spec
  0022 Abschnitt 3) und übergibt danach nur noch die fertige
  `AiProvider`-Instanz in den Hintergrund-Task — kein wiederholter
  Store-Zugriff.
- Ein dauerhaft fehlschlagender Provider blockiert nichts: die gespeicherte
  Notiz bleibt unangetastet, der Nutzer bekommt eine klare Fehlermeldung
  (`note-shrink-failed`), kein Hang (`NOTE_SHRINK_CALL_TIMEOUT`, dieselbe
  Absicherung wie `compaction::SUMMARY_CALL_TIMEOUT`).
- `ManagementView`/`ServerForm`/`NotesPanel` bleiben für den regulären
  Navigationspfad unverändert — der neue `initialSelection`-Prop ist rein
  additiv (Default `null`).
