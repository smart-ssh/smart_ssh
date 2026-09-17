# 0051 — Notiz-/Server-UI-Politur (Spec 0058): offen gelassene Entscheidungen

## Status

Angenommen

## Kontext

Spec 0058 ("Notiz- & Server-UI-Politur", Session-Modell Etappe 5 + zwei
Etappe-4-Reste + ein Pseudo-Server-Kosmetikpunkt) lässt mehrere
Umsetzungsdetails offen bzw. wirft während der Implementierung neue Fragen
auf, die hier festgehalten werden — CLAUDE.md verlangt einen ADR "besonders
[bei] einer Scope-Reduktion", was für Teil 3 wörtlich zutrifft.

## Entscheidungen

### 1. Teil 3 ("Port 0") — kein Commit, bereits behoben

Der von Spec 0058 §3 beschriebene Bug (der lokale Pseudo-Server zeigt in
der Server-Liste „Port 0" statt gar keinen Port) existierte bereits, wurde
aber **vor** diesem Politur-Paket unter Spec 0046 behoben — Commit
`9870682` ("fix(frontend): hide port for local pseudo-server per spec
0046"), mit eigenem, weiterhin grünem Regressionstest
(`ServerList.test.tsx`, `describe("ServerList port display (Spec 0046,
Fund 5)")`). Vor dem Beginn dieses Pakets wurde jede Stelle im Frontend
durchsucht, die `ServerDto.port` referenziert (`ServerForm.tsx`,
`HostKeyDialog.tsx`, `Sidebar.tsx`, `SessionTabBar.tsx`,
`FileBrowserPanel.tsx`, `GroupForm.tsx`, `ChatSessionPickerScreen.tsx`) —
keine weitere Stelle zeigt den Wert unbedingt an. Mit dem Nutzer
abgestimmt: kein eigener Commit für Teil 3, da nichts mehr zu beheben ist.

### 2. "Jetzt zusammenfassen" nur für Server-Notizen, nicht für Gruppen

Der optionale Link im proaktiven Hinweis (§1) ruft denselben
KI-Kürzungs-Fluss wie der Sitzungsende-Dialog (Etappe 4,
`commands::request_note_shrink`) auf. Dieser Fluss ist strukturell auf
Server beschränkt: `AiAction::ProposeNoteUpdate`s `NoteTargetSelector`
kennt zwar `CurrentServer` und `CurrentServerGroup`, aber `execute_note_
shrink_request`/`NoteShrinkTarget` sind bewusst — wie schon der gesamte
Etappe-4-Dialog (Spec 0057 §4.2, wörtlich "Notiz für **diesen Server**")
— nur für den Server-Fall gebaut. Eine Gruppen-Erweiterung hätte den
Rahmen von "wenn einfach" (Spec 0058 §1) gesprengt; `NotesPanel.tsx`
zeigt den Link deshalb nur, wenn `"Server" in target`.

### 3. Lokaler Pseudo-Server: Kürzung ohne Revisions-Historie bleibt unwiderruflich

spec-reviewer-Fund (Review dieses Pakets): Mit dem Fix aus Teil 2 kann der
komplette Etappe-4-Kürzungs-Fluss jetzt auch für den lokalen Pseudo-Server
laufen (`local_server::LocalNoteShrinkTarget`). Anders als bei einem
echten Server — dort landet jede Zustimmung als neue `NoteRevision`
(rückholbar über "Historie anzeigen"/Wiederherstellen) — schreibt
`LocalNoteShrinkTarget::write` über `local_server::save_notes` direkt in
den `tauri-plugin-store`, **ohne jede Historie** (dieselbe bewusste
Design-Entscheidung wie beim manuellen Speichern der lokalen Notiz, s.
`local_server.rs`-Moduldoc: "keine Notiz-Historie für den lokalen
Pseudo-Server, nur der aktuelle Stand"). Eine akzeptierte KI-Kürzung der
lokalen Notiz ist damit **endgültig** — vor diesem Paket war das
ungefährlich, weil der Accept-Pfad für den lokalen Server strukturell
immer fehlschlug (er kam nie so weit). Kein Spec-Verstoß (Spec 0057 §4.2
verlangt nur Nutzer-Bestätigung, keine Undo-Garantie, und die Bestätigung
selbst — der Diff-Dialog — bleibt unverändert Pflicht), aber eine
Asymmetrie zum Server-Fall, die hier bewusst dokumentiert statt in dieser
Runde behoben wird (eine Historie für den lokalen Server nachzurüsten wäre
eine eigenständige, größere Erweiterung von Spec 0032, nicht Teil dieses
Politur-Pakets).

### 4. Neues `note-shrink-succeeded`-Event

spec-reviewer-Fund (Review dieses Pakets): Vor diesem Fix gab es kein
Erfolgs-Gegenstück zu `note-shrink-failed`. Das war in Etappe 4 harmlos
(der Anstoß kam immer aus einem Toast nach `disconnect()`, der Editor war
zu dem Zeitpunkt typischerweise nicht offen), wird aber durch den neuen
"Jetzt zusammenfassen"-Link **im offenen Editor selbst** (§1) zu einem
echten Problem: ohne Neuladen hätte ein nachfolgender Klick auf "Speichern"
die gerade akzeptierte Zusammenfassung mit dem alten Entwurf
überschrieben. `execute_note_shrink_request` emittiert jetzt zusätzlich
`note-shrink-succeeded`; `ServerForm` lädt bei einem Treffer den Server
neu (`loadServer()`), was über die `currentNotes`-Prop automatisch auch
`NotesPanel`s Entwurf zurücksetzt — keine zusätzliche Logik dort nötig.

## Konsequenzen

- Kein neuer Commit für Teil 3 — die Spec-Reihenfolge ("1. Teil 3 (Port
  0) — kleinster, isoliert") wird dadurch übersprungen, ohne dass ein
  Fix fehlt.
- Gruppen-Notizen bleiben ohne "Jetzt zusammenfassen"-Komfort — Nutzer
  können sie weiterhin nur manuell kürzen (unverändert gegenüber vor
  diesem Paket).
- Der lokale Pseudo-Server braucht besondere Sorgfalt beim Formulieren
  künftiger Hinweistexte zu KI-Kürzung (kein "kannst du jederzeit
  rückgängig machen"-Versprechen, das für ihn nicht gilt).
