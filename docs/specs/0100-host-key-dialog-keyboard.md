# Spec 0100 — Host-Key-Dialog: modal angesagt, Fokus gefangen, Escape lehnt ab

Status: umgesetzt
Zweck: Die Host-Key-Abfrage (unbekannter und geänderter Schlüssel) ist allein mit der Tastatur sicher bedienbar — beim Öffnen liegt der Fokus auf der ablehnenden Schaltfläche, Tab verlässt den Dialog nicht, Escape lehnt ab, und Screenreader sagen ihn als modalen Dialog an.
Bezüge: Spec 0007 (Host-Key-Prüfung), ADR 0091 (Fokus-Fang und Escape), ADR 0104 (Dialog an der App-Wurzel), ADR 0116 (`jsx-a11y` als Fehler).
Review-Priorität: ERHÖHT (Bestätigung einer Verbindung)

## 1. Der Dialog

Die Abfrage erscheint in zwei Ausprägungen:

- **Unbekannt** (Schlüssel zum ersten Mal gesehen): Schaltflächen
  „Ablehnen" und „Vertrauen".
- **Geändert** (Schlüssel weicht vom bekannten ab): rot gestaltet, mit
  Gegenüberstellung von bekanntem und angebotenem Fingerabdruck und den
  Schaltflächen „Verbindung abbrechen" und „Trotzdem vertrauen".

In beiden Fällen steht die ablehnende Schaltfläche zuerst. Der Dialog liegt
über allem anderen, auch wenn die Abfrage aus einem ausgeblendeten Bereich
der Oberfläche ausgelöst wird (etwa ein Verbindungsaufbau, während ein
anderer Sitzungs-Tab aktiv ist); er ist nie unsichtbar oder unerreichbar. Er
wird aus der Server-Liste (Verbindungsaufbau) und aus dem Server-Formular
(Verbindungstest) geöffnet.

Ein Klick auf den Hintergrund entscheidet nichts.

## 2. Anforderungen

**A1 Ansage.** Das Dialogfenster ist im Zweig „geändert" ein
`alertdialog`, im Zweig „unbekannt" ein `dialog`, jeweils modal
(`aria-modal`). Sein zugänglicher Name ist die sichtbare Überschrift, seine
Beschreibung der erklärende Text.

**A2 Anfangsfokus.** Beim Öffnen liegt der Fokus auf der ablehnenden
Schaltfläche, in beiden Zweigen. Ein Enter direkt nach dem Öffnen lehnt
also ab. Das gilt auch, wenn der Dialog ein neues Ereignis oder einen
anderen Zweig anzeigt (unbekannt → geändert): der Fokus liegt danach wieder
auf der ablehnenden Schaltfläche des aktuellen Zweigs, nie auf „Vertrauen"
oder dem Hintergrund.

**A3 Fokus-Fang.** Tab von der letzten und Shift+Tab von der ersten
fokussierbaren Stelle bleiben im Dialog (der Fokus springt zyklisch). Gelangt
der Fokus auf andere Weise nach außen (Klick auf den Hintergrund,
Fokusverlust, programmatischer Fokus), wird er in den Dialog zurückgeholt,
und zwar auf die ablehnende Schaltfläche.

**A4 Escape.** Escape löst genau einmal die Entscheidung „ablehnen" aus,
nie „vertrauen". Das gilt auch, wenn der Fokus nicht im Dialog liegt (etwa
weil das fokussierte Element entfernt wurde und der Fokus auf den
Dokumentkörper fiel). Escape erreicht keine dahinterliegenden Tastatur-
Handler der Anwendung, solange der Dialog offen ist. Zeigt der Dialog nach
einem überlappenden Ereigniswechsel eine neue Abfrage, gilt Escape für die
aktuell angezeigte, nie für die ersetzte.

**A5 Keine Entscheidung ohne Absicht.** Weder ein Klick auf den
Hintergrund noch Fokusverlust noch ein anderer Weg, den Dialog zu verlassen,
löst „vertrauen" aus. Ein Enter mit Auto-Repeat (die Taste wird aus einem
vorherigen Schritt noch gehalten) löst keine Entscheidung aus und wird
verworfen. Ein frisches Enter bleibt der normalen Bedienung der fokussierten
Schaltfläche überlassen. Die Leertaste ist ausgenommen: Sie löst eine
Schaltfläche erst beim Loslassen aus, und da der Fokus beim Öffnen auf der
ablehnenden Schaltfläche liegt, wird schlimmstenfalls abgelehnt. Pro
Abfrage wird höchstens eine Entscheidung gemeldet; ein Klick nach Escape
löst keine zweite aus.

**A6 Linter.**
- **A6.1** Die Regeln des Linters für Zugänglichkeit (`jsx-a11y`) sind
  aktiv, und jede ihrer Regeln steht auf Fehler. Ein neuer Fund lässt den
  Lint-Schritt und damit den CI-Job scheitern; das Frontend hat auf `main`
  keinen Fund. Ausnahmen gelten in jeder Komponente wie in A6.3.
- **A6.3** Der Host-Key-Dialog hat keinen `jsx-a11y`-Fund. Nötige
  Ausnahmen sind zeilengenau und begründet unterdrückt, keine Regel ist
  global abgeschaltet.

(A6.2 und A8 sind entfallen; die Kennungen werden nicht neu vergeben.)

**A7 Fokus zurück.** Nach der Entscheidung kehrt der Fokus auf das Element
zurück, das ihn vor dem Öffnen hatte, sofern es noch existiert.

## 3. Sicherheitszusagen

- **Abbruch lehnt ab, er gewährt nie.** Escape, Fokusverlust,
  Hintergrund-Klick und gehaltene Tasten führen höchstens zu „ablehnen"
  (A4, A5). Läuft die Bestätigungsfrist der Abfrage ab, wird ebenfalls
  abgelehnt (Verbindung scheitert mit `SSH_HOST_KEY_CONFIRM_TIMEOUT`).
- **Die sichere Wahl ist die bequemste.** Der Anfangsfokus liegt auf der
  ablehnenden Schaltfläche (A2).
- Filter, Redaction und Credentials sind von diesem Verhalten nicht berührt.

## 4. Prüffälle

Die Komponententests verweisen mit diesen Kennungen auf die Anforderungen;
jeder Fall läuft für beide Zweige.

- **T1** Ansage (A1).
- **T2** Anfangsfokus, auch nach Ereignis- und Zweigwechsel (A2).
- **T3** Frisches Enter bleibt unangetastet, Fokus bleibt auf „ablehnen"
  (A2, A5).
- **T4** Tab-/Shift+Tab-Fang (A3).
- **T5** Escape lehnt genau einmal ab und erreicht keinen Handler auf
  `window` (A4).
- **T6** Gehaltenes Enter wird verworfen (A5).
- **T7** Hintergrund-Klick und Fokusverlust entscheiden nichts, der Fokus
  kehrt in den Dialog zurück (A3, A5).
- **T8** Klick auf jede Schaltfläche löst genau ihre Entscheidung aus.
- **T9** Der Lint-Lauf ist mit aktivem `jsx-a11y` fehlerfrei; ein
  Fund lässt ihn scheitern (A6).
- **T10** Fokus kehrt nach der Entscheidung zurück, auch unter React
  StrictMode (A7).
- **T12** Escape nach überlappendem Ereigniswechsel gilt dem neuen Ereignis
  (A4). Die Kennung T11 wird nicht verwendet.

## 5. Grenzen

- Nur der Host-Key-Dialog trägt Fokus-Fang und Escape-Verhalten; andere
  Dialoge sind nicht Teil dieser Spec.
- Die Tests laufen in `jsdom`, das Enter auf Schaltflächen nicht aktiviert
  und Tab nicht ausführt; die eigentliche Aktivierung und das Ansagen durch
  einen Screenreader zeigt nur die echte Webview (Handtest).
