# Spec: Häufigere Notiz-Vorschläge mit Änderungs-Vorschau

Status: Entwurf
Modul: Erweiterung `crates/app-tauri` (Kernschleife, Events),
`frontend/` (Bestätigungsdialog, Disconnect-Benachrichtigung)
Abhängigkeiten: Server-/Notiz-Datenmodell (Spec 0003, Abschnitt 5),
Kernschleife (Spec 0007, Abschnitt 6), Notiz-Vorschlag beim Beenden
(Spec 0010)

## 1. Ausgangslage — zwei Lücken

**1a. Zu zurückhaltende KI-Instruktion.** Der System-Prompt (s.
`crate::commands::build_session_system_context`) enthält aktuell nur einen
einzeiligen, unauffälligen Hinweis ("Wenn wichtige permanente Erkenntnisse
... festgehalten werden sollen, schlage eine Notiz-Aktualisierung vor") —
das führt in der Praxis dazu, dass die KI Notiz-Vorschläge selten macht,
meist nur am Sitzungsende (Spec 0010) statt während der Sitzung, sobald
tatsächlich etwas Neues gelernt wurde.

**1b. Fehlende Änderungs-Vorschau, entgegen der ursprünglichen Spec.**
Spec 0003, Abschnitt 5.2 legt bereits fest: *"`ProposeNoteUpdate` wird
immer als Diff-Ansicht (alt/neu) angezeigt, nie automatisch übernommen"*.
Umgesetzt ist davon nur die "nie automatisch übernommen"-Hälfte:

- Der reguläre In-Chat-Vorschlag (`chat-action-proposed` mit
  `ProposeNoteUpdate`, gerendert von `ChatPanel`) zeigt aktuell **gar
  keine** Vorschau — nur ein Label ("Notiz aktualisieren (dieser
  Server)") plus Ausführen/Ablehnen, ohne dass der Nutzer sehen kann, was
  sich inhaltlich ändert.
- Die Disconnect-Benachrichtigung (`note-update-suggested`, Spec 0010,
  gerendert von `NoteSuggestionToast`) zeigt zwar den vollen neuen Text,
  aber keinen Diff gegen den bisherigen Inhalt — bei einer längeren
  bestehenden Notiz mit einer kleinen Ergänzung muss der Nutzer den
  gesamten Text erneut querlesen, um die eigentliche Änderung zu finden.

Diese Spec schließt beide Lücken.

## 2. System-Prompt: proaktivere Instruktion

Der bestehende Hinweis in `build_session_system_context` wird ersetzt durch
eine Formulierung, die aktives, mehrfaches Vorschlagen **während** der
Sitzung nahelegt, sobald neue, wiederverwendbare Information anfällt
(installierte Software/Versionen, Konfigurationspfade, getroffene
Entscheidungen, behobene Probleme, Systembesonderheiten) — nicht erst am
Ende abwarten. Der bereits an anderer Stelle (Spec 0010, Abschnitt 2)
etablierte Anti-Spam-Grundsatz ("keine Wiederholung bereits bestehender
Notizinhalte") wird explizit mit in den Haupt-System-Prompt übernommen,
damit "häufiger" nicht mit "redundanter" verwechselt wird.

Kein neuer Mechanismus, reine Prompt-Textänderung — die KI entscheidet
weiterhin selbst pro Situation, `ProposeNoteUpdate` bleibt unverändert
immer bestätigungspflichtig (Spec 0003, Abschnitt 5.2).

## 3. Änderungs-Vorschau: Backend

`AiAction::ProposeNoteUpdate` selbst (Spec 0003, Abschnitt 5.2) bleibt
unverändert — die KI liefert weiterhin nur `target`/`new_content`, keinen
Diff (sie könnte einen selbst formulierten "Änderungs-Kommentar" ungenau
oder irreführend zusammenfassen; ein serverseitig **berechneter** Diff
gegen den tatsächlich gespeicherten Inhalt ist zuverlässiger und passt zum
Transparenzprinzip — nichts, das der Nutzer bestätigt, basiert auf einer
unverifizierten KI-Selbstauskunft).

Stattdessen wird der *aktuelle* Inhalt des aufgelösten Ziels als
zusätzliches, rein präsentationsbezogenes Feld an die beiden betroffenen
Events angehängt:

```
chat-action-proposed    { ..., previousNoteContent: string | null }
note-update-suggested   { ..., previousNoteContent: string | null }
```

`null`, falls die Zielauflösung fehlschlägt (z. B. Server zwischenzeitlich
gelöscht) oder das Ziel bisher gar keinen Inhalt hat (neue, leere Notiz) —
das Frontend zeigt dann den neuen Inhalt ohne Diff-Hervorhebung, aber nie
einen harten Fehler. Berechnet wird das an der Stelle, an der das Event
ohnehin schon gebaut wird (`crate::orchestration::handle_action_proposed`
bzw. `suggest_note_update_on_disconnect`), per direktem
`ProfileStore::get_server`/`get_group` auf den bereits vorhandenen
`resolve_note_target`-Mechanismus — kein neuer Tauri-Command nötig.

## 4. Änderungs-Vorschau: Frontend

Ein einfacher zeilenbasierter Diff (kein externes Paket, reine, kurze
TS-Funktion) zwischen `previousNoteContent` und `new_content` — dargestellt
**kurz**: unveränderte Zeilen werden weggelassen (nicht der gesamte
bestehende Notiztext erneut gezeigt), hinzugefügte Zeilen grün, entfernte
Zeilen rot/durchgestrichen, konsistent mit der bestehenden Ampel-Palette
(Spec 0009). Beide Stellen bekommen dieselbe Darstellung über eine
gemeinsame Komponente:

- `ChatPanel`s Aktions-Karte für `ProposeNoteUpdate` — bisher ganz ohne
  Vorschau (Abschnitt 1b), bekommt sie neu.
- `NoteSuggestionToast` — ersetzt die bisherige Rohtext-`<pre>`-Anzeige
  durch denselben Diff, im bereits bestehenden "Anzeigen"-aufgeklappten
  Zustand.

## 5. Nicht-Ziele

- Kein Wort-/Zeichen-genauer Diff (nur zeilenbasiert) — für Notiztexte
  (kurze, meist listenartige Einträge) ausreichend und deutlich einfacher
  als ein vollwertiger Text-Diff-Algorithmus.
- Keine Änderung an `record_revision`/der Notiz-Historie selbst (Spec
  0008, Abschnitt 6) — die Vorschau ist rein für den Bestätigungsdialog,
  die gespeicherte Revision bleibt wie bisher der volle neue Text.

## 6. Offene Punkte

- Schwellenwert/Heuristik, ab welcher Änderungsgröße eine Notiz-
  Aktualisierung wirklich sinnvoll ist (vs. Rauschen durch zu triviale
  Vorschläge), bleibt bewusst der KI-Instruktion selbst überlassen
  (Abschnitt 2) statt einer harten serverseitigen Regel — eine feste
  Schwelle (z. B. "mindestens N neue Zeichen") wäre eine willkürliche
  Grenze ohne echten Anhaltspunkt für den richtigen Wert.
