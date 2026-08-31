# Spec: Automatischer Notiz-Vorschlag beim Beenden einer Session

Status: Entwurf
Modul: Erweiterung `crates/app-tauri` (Session-Lifecycle aus Spec 0007)
Abhängigkeiten: `AiAction::ProposeNoteUpdate` (Spec 0003, Abschnitt 5.2),
`AiProvider` (Spec 0006), bestehender Bestätigungsdialog aus Spec 0007

## 1. Ziel

Beim Beenden einer Server-Session (Klick auf "Beenden"/Disconnect) wird die
KI **einmalig und gezielt** gefragt, ob es aus dem Sitzungsverlauf
festhaltenswerte Informationen für die Server-Notiz gibt (neue Pfade,
installierte Versionen, getroffene Entscheidungen). Der Vorschlag erscheint
im bereits bestehenden Diff-Bestätigungsdialog — keine neue UI-Komponente,
Wiederverwendung des Mechanismus aus Spec 0003/0007.

## 2. Ablauf

1. Nutzer klickt "Beenden". Die SSH-Verbindung wird **sofort** getrennt
   (`disconnect()`, Spec 0005/0007) — nicht auf die KI-Antwort warten, das
   würde den eigentlichen Trennvorgang unnötig verzögern.
2. Parallel/danach: Backend ruft `AiProvider::send()` **einmalig** mit dem
   bisherigen `SessionContext` der Session auf, ergänzt um eine gezielte
   Abschluss-Instruktion (System-/Kontext-Ergänzung, kein sichtbarer
   Chat-Eintrag): sinngemäß "Gibt es aus dieser Sitzung Informationen, die
   für künftige Sitzungen an diesem Server als Notiz festgehalten werden
   sollten? Nur bei echtem Mehrwert vorschlagen, keine Wiederholung
   bestehender Notizinhalte."
3. **Wichtig**: `available_actions` wird für diesen einen Aufruf auf
   `ProposeNoteUpdate` beschränkt (Spec 0006, Abschnitt 3) — die KI soll in
   diesem Moment keine Shell-Kommandos vorschlagen können, nur eine
   Notiz-Aktualisierung.
4. Liefert die KI kein `ActionProposed`-Event (entscheidet sich gegen einen
   Vorschlag) oder schlägt ein `AiError` fehl: **kein Dialog**, die Session
   schließt einfach ohne weitere Nachfrage. Das ist der erwartete Regelfall,
   wenn nichts Neues passiert ist — kein aufdringliches "leerer Vorschlag"-Popup.
5. Liefert die KI ein `ProposeNoteUpdate`: derselbe Diff-Bestätigungsdialog
   wie bei einem regulären KI-Notizvorschlag während des Chats (Spec 0007,
   Abschnitt 6, letzter Punkt) erscheint — alt/neu, Annehmen/Ablehnen. Bei
   Annahme: `record_revision(..., NoteEditor::Ai { provider, model })`,
   identisch zum bestehenden Mechanismus, keine Sonderbehandlung.
6. Der Dialog erscheint auch dann noch, wenn der Nutzer inzwischen schon zu
   einem anderen Screen navigiert hat (z. B. als dezente Benachrichtigung
   statt eines blockierenden Modals) — die SSH-Verbindung ist zu diesem
   Zeitpunkt ja bereits getrennt, das Ergebnis kommt asynchron nach.

## 3. Abgrenzung

- Der Vorschlag bezieht sich **nur auf die Notiz des Servers selbst**, nicht
  auf übergeordnete Gruppen-Notizen — sonst müsste die KI raten, auf welcher
  Hierarchieebene eine Information "richtig" aufgehoben ist, was zu
  inkonsistenten Ablagen führen könnte. Gruppen-Notizen bleiben weiterhin
  ausschließlich manuell bzw. über explizite Chat-Vorschläge editierbar.
- Kein automatischer Trigger bei sehr kurzen Sessions ohne ausgeführte
  Kommandos (z. B. Verbindung sofort wieder getrennt) — Schwelle: mindestens
  ein erfolgreich ausgeführtes Kommando in der Session, sonst wird der
  KI-Aufruf gar nicht erst gemacht (spart unnötige API-Kosten für einen
  Vorschlag, der ohnehin nichts liefern würde).

## 4. Offene Punkte

- Soll es eine Nutzer-Einstellung geben, dieses automatische Nachfragen
  global zu deaktivieren (manche Nutzer wollen vielleicht nie automatische
  Vorschläge, nur explizite während des Chats)? Naheliegend als kleiner
  Schalter in den Einstellungen, aber nicht zwingend Teil dieser Spec.
