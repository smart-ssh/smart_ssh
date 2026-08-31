# 0017-note-suggestion-on-disconnect-design

## Status
Accepted

## Kontext

`docs/specs/0010-note-suggestion-on-disconnect.md` beschreibt Ablauf und
Schwelle des automatischen Notiz-Vorschlags beim Beenden einer Session
genau, lässt aber drei konkrete Umsetzungsfragen offen: über welchen
Event-Mechanismus der Vorschlag das Frontend erreicht, wie er dort
dargestellt wird, und wie die Bestätigung technisch abläuft, obwohl die
Session zu diesem Zeitpunkt bereits getrennt ist.

## Entscheidungen

**1. Neues `note-update-suggested`-Event statt Wiederverwendung von
`chat-action-proposed`.** Beide tragen dieselbe fachliche Bedeutung (ein
`AiAction::ProposeNoteUpdate`-Vorschlag), aber `chat-action-proposed` wird
im Frontend ausschließlich von der `ChatPanel`-Instanz einer *offenen*
Session konsumiert (`if (event.sessionId !== sessionId) return;` — an eine
bestimmte Screen-Instanz gebunden). Spec 0010 Abschnitt 2, Punkt 6 verlangt
aber ausdrücklich, dass der Vorschlag **auch dann noch ankommt, wenn der
Nutzer den Screen bereits verlassen hat** — ein an eine Screen-Instanz
gebundener Listener kann das strukturell nicht leisten, unabhängig davon,
wie er implementiert ist. Das neue Event wird deshalb app-weit in `App.tsx`
abonniert (`NoteSuggestionToast`), nicht innerhalb eines bestimmten Screens.
Der Payload ist bewusst schlanker als `chat-action-proposed` (kein
`decision`-Feld) — `ProposeNoteUpdate` verlangt ohnehin immer `Confirm`
(Spec 0003, Abschnitt 5.2), das Feld wäre hier immer derselbe konstante
Wert.

**2. Nicht-blockierende Toast-Karte statt Modal, mit Inline-Ausklappen
statt separatem Dialog.** Spec 0010 Abschnitt 2, Punkt 6 schließt ein
blockierendes Modal ausdrücklich aus ("dezente Benachrichtigung"), lässt
die konkrete Darstellung aber offen. Umgesetzt als kompakte Karte unten
rechts (`NoteSuggestionToast.tsx`), die im Standardzustand nur Zielart
("Server"/"Gruppe") zeigt; ein Klick auf "Anzeigen" klappt dieselbe Karte
zum vollen Vorschlag (Ziel-ID, neuer Notizinhalt, Annehmen/Ablehnen) auf,
statt eine zweite, separate Dialog-Komponente über den aktuellen Screen zu
legen — letzteres wäre der Sache nach doch wieder ein Modal.

**3. `respond_to_action`s bisheriger `state.sessions.get(session_id)`-Check
entfernt.** Die Annahme/Ablehnung des Vorschlags läuft über denselben
`respond_to_action`-Command wie ein regulärer In-Chat-Vorschlag (Spec 0010,
Abschnitt 2, Punkt 5: "identischer Ablauf") — die Session ist zu diesem
Zeitpunkt aber per Design bereits aus `AppState.sessions` entfernt
(`disconnect()` hat sie vor dem Start des Hintergrund-Tasks herausgenommen).
Der bisherige Check hätte diesen gültigen Aufruf fälschlich mit "Session
nicht gefunden" abgelehnt. Der Check war ohnehin redundant:
`ConfirmationRegistry::resolve()` prüft die Gültigkeit von `action_id`
bereits selbst und liefert einen eigenen, klaren Fehler für eine
unbekannte oder bereits aufgelöste ID.

## Konsequenzen

**Positiv:**
- Der Vorschlag erreicht den Nutzer zuverlässig unabhängig davon, welchen
  Screen/Tab er gerade offen hat — exakt die von der Spec verlangte
  Eigenschaft.
- Kein zweiter, paralleler Bestätigungsmechanismus: Annahme/Ablehnung läuft
  über exakt denselben Command und exakt dieselbe `execute_note_update`-
  Funktion wie ein regulärer In-Chat-Notizvorschlag.
- Der Toast bleibt bewusst einfach (kein echtes Diff, nur der neue Inhalt)
  — konsistent mit dem bereits bestehenden In-Chat-Bestätigungsdialog für
  `ProposeNoteUpdate` (Spec 0007 Teil 2), der ebenfalls keinen echten
  Alt/Neu-Vergleich zeigt, nur den Vorschlag selbst.

**Negativ / Trade-off:**
- Zwei strukturell ähnliche, aber getrennte Events (`chat-action-proposed`
  und `note-update-suggested`) für dieselbe zugrunde liegende `AiAction`-
  Variante — ein Frontend-Entwickler muss wissen, dass beide existieren und
  wofür welches gedacht ist.
- Der gelockerte `respond_to_action`-Check bedeutet: ein Aufruf mit einer
  `action_id`, die zu einer Aktion aus einer noch **laufenden** Session
  gehört, wird nicht mehr zusätzlich über die Session-Existenz abgesichert
  — er war es aber ohnehin nur zusätzlich zur (bereits ausreichenden)
  `resolve()`-eigenen Prüfung, kein Sicherheitsverlust in der Praxis.
- Mehrere gleichzeitig offene Toasts (mehrere Sessions kurz hintereinander
  beendet) stapeln sich unten rechts ohne Limit — bei der zu erwartenden
  Nutzung (eine Session nach der anderen) unproblematisch, könnte bei sehr
  vielen parallelen Verbindungen unübersichtlich werden.
