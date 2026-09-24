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

### 2. Schwellwert: `LARGE_NOTE_DIALOG_THRESHOLD_CHARS = 10_000`

Deutlich über der Zusammenfassungs-Obergrenze `NOTE_SHRINK_MAX_BYTES`
(4_000 Byte), damit normal genutzte Notizen (typischerweise wenige hundert
Zeichen) nicht nerven (§4.2, wörtlich: "Nur bei großer Notiz — bei normalen
Notizen kein Dialog"). Gezählt werden Unicode-Skalarwerte (Zeichen), nicht
Byte — Spec 0079 stellt die ursprüngliche Byte-Schwelle (8_000) auf Zeichen
um, überall dort, wo die Konstante wirkt (dieser Dialog und der proaktive
Hinweis im Notiz-Editor).

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

### 6. Nacharbeiten aus dem `spec-reviewer`-Review dieses Schritts

Der pflichtgemäße `spec-reviewer`-Durchlauf (CLAUDE.md, ERHÖHT) fand keinen
Bruch der zentralen Invariante (gespeicherte Notiz wird nie ohne
Diff-Bestätigung verändert — beide `persist_note_revision`-Aufrufer stehen
hinter einem aufgelösten `Approve`/`EditThenApprove`), aber mehrere
Sicherheits-/Robustheits-Lücken. Behoben, in separatem Commit:

- **Notiz ungefenced im Kürzungs-Prompt.** Spec 0039 §3 verlangt
  `fence_untrusted` für jede der vier untrusted Quellen, Server-/
  Gruppen-Notizen eingeschlossen — `compaction::compact_for_send` tut das
  beim Senden bereits. `summarize_note_for_shrink` bettete die Notiz aber
  roh in den Prompt ein. Relevant, weil eine Notiz über "In Notiz
  übernehmen" (Spec 0040 §6) oder einen angenommenen KI-Notiz-Vorschlag
  Inhalt tragen kann, der ursprünglich von einem Remote-Host stammte —
  eine darin eingeschleuste Instruktion wäre sonst als gleichrangiger
  Prompt-Text statt als Daten gelesen worden. Jetzt `fence_untrusted
  (UntrustedKind::ServerNote, &server.name, &redacted_note)`, exakt wie im
  Sende-Pfad.
- **Schwächerer Redactor als im Session-Pfad.** `request_note_shrink` baute
  bisher einen einfachen `DefaultOutputRedactor::new()` — ohne das
  session-spezifische Sudo-Passwort-Muster, das `connect()` für genau den
  Fall aufbaut, dass `sudo` (NOPASSWD/gültiger Timestamp) das Passwort
  ungefiltert an die ausgeführte Kommandoausgabe durchreicht. Landet eine
  solche Ausgabe über "In Notiz übernehmen" in der Notiz, hätte die
  generische Musterliste ein nacktes Passwort nicht erfasst. Jetzt baut
  `request_note_shrink` denselben `with_extra_patterns`-Redactor wie
  `connect()`.
- **Stiller Fehlschlag nach Zustimmung.** Schlug `persist_note_revision`
  NACH einem `Approve` fehl (DB gesperrt, Server zwischenzeitlich
  gelöscht), gab es nur ein `tracing::warn!` — der Nutzer hätte "Annehmen"
  geklickt, die Karte wäre verschwunden, und er hätte angenommen, die
  Notiz sei jetzt gekürzt, obwohl nichts geschrieben wurde. Jetzt zusätzlich
  ein `note-shrink-failed`-Event.
- **Verlorene zwischenzeitliche Änderung (TOCTOU).** Das
  Bestätigungsfenster ist bis zu `PENDING_ACTION_CONFIRM_TIMEOUT` (3600s)
  lang — genug Zeit, dass der Nutzer die Notiz in der Zwischenzeit selbst
  ändert. Ein blindes Überschreiben mit der KI-Zusammenfassung hätte diese
  Änderung verloren, obwohl der Nutzer nur einem Diff gegen den ALTEN Stand
  zugestimmt hatte — eine formale Verletzung von "nie ohne Bestätigung
  verändert" (bestätigt wurde ein Diff, der nicht mehr dem aktuellen Stand
  entsprach). `execute_note_shrink_request` liest den Server jetzt
  unmittelbar vor dem Schreiben erneut, vergleicht gegen den beim
  KI-Aufruf gelesenen Stand, und bricht bei Abweichung mit
  `note-shrink-failed` ab, statt zu überschreiben.
- **Fehlender Kürzungs-Hinweis.** Kappt `NOTE_SHRINK_MAX_BYTES` eine zu
  lange KI-Antwort, sah der Nutzer im Diff bisher eine mitten im Satz
  abbrechende Notiz ohne erkennbaren Grund. Jetzt ein kurzer Hinweis-Zusatz
  (innerhalb desselben Byte-Caps, s. `summarize_note_for_shrink`).
- **React-Duplicate-Key.** Erschien derselbe Server zweimal hintereinander
  (z. B. weil die vorherige Karte nie beantwortet wurde), erzeugte
  `NoteShrinkSuggestionToast` zwei Einträge mit identischem
  `key={serverId}`. Jetzt beim Einfügen dedupliziert (ersetzt statt
  angehängt).
- **`pendingNoteEditSelection` wurde nie zurückgesetzt.** Nach "Mache ich
  selbst" blieb die Ziel-Auswahl in `App.tsx` dauerhaft gesetzt — ein
  SPÄTERER manueller Wechsel zu "Verwalten" (nach Verlassen/Zurückkommen,
  `ManagementView` wird dabei unmounted) wäre erneut zu demselben Server
  gesprungen. `ManagementView` bekommt jetzt einen
  `onInitialSelectionConsumed`-Rückkanal, der die Auswahl in `App.tsx`
  nach der Übernahme löscht.

Bewusst NICHT behoben, dem Nutzer explizit gemeldet:

- **"Mache ich selbst" springt zu `ServerForm` (das den `NotesPanel`
  enthält), nicht mit Fokus/Scroll direkt auf das Notizfeld.** §4.2 sagt
  nur "öffnet die Notiz-Bearbeitung", ohne die genaue Zielgranularität
  vorzuschreiben — bei einer wirklich langen Notiz muss der Nutzer im
  Formular etwas scrollen. Kein Sicherheitsproblem, reiner UX-Feinschliff
  für eine spätere Iteration.
- **Der lokale Pseudo-Server (`LOCAL_SERVER_ID`) bekommt den Dialog nie**
  (`suggest_note_shrink_on_disconnect`s `profile_store.get_server(...)`
  schlägt für ihn strukturell fehl, da er keine `servers`-Zeile hat, s.
  `local_server.rs`) — eine emergente Lücke, kein verbotenes
  `if server_id == LOCAL_SERVER_ID` im Sicherheitspfad (das wäre laut
  CLAUDE.md ohnehin unzulässig). Auswirkung ist rein kosmetisch (der
  lokale Server bekommt den Komfort-Vorschlag nicht), kein
  Daten-/Sicherheitsproblem — nicht in dieser Runde behoben.
- **Kein Rate-Limit/keine Entprellung für `request_note_shrink`** — ein
  direkter, wiederholter Aufruf (z. B. aus den DevTools) könnte mehrere
  KI-Aufrufe/Diff-Karten für denselben Server erzeugen. Kein
  Bestätigungs-Bypass (die Persistenz bleibt in jedem Fall hinter dem
  Diff-Dialog), nur ein potenzieller Kostenpunkt — dieselbe Klasse
  Kompromiss wie bei jedem anderen ungedrosselten Tauri-Befehl in diesem
  Projekt, nicht spezifisch für diesen Schritt.

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
