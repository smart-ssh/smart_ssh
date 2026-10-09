# Spec 0009 — Filter-Regel-Verwaltung

Status: umgesetzt
Zweck: Nutzer legen Filterregeln dauerhaft an und verwalten sie, sehen die fest eingebaute Hard-Blacklist und können vorab prüfen, wie ein Beispielkommando bewertet würde.
Bezüge: Spec 0002 (Filter-Engine), Spec 0037 (Herkunft von Regeln), Spec 0077 (ungültige Muster), Spec 0007 (Kernschleife), Spec 0008 (keine Drag-and-Drop-Bedienung).

## 1. Ziel

Regeln wie „`ls *` immer erlauben" oder „`systemctl *` auf
Produktionsservern immer bestätigen" bleiben über Neustarts erhalten. Die
gespeicherten Regeln fließen in jede Auswertung der KI-Kommandoschleife ein.
Ohne gespeicherte Regeln läuft praktisch jeder Vorschlag auf den Standardfall
`Confirm` hinaus (Spec 0002, Abschnitt 3.3).

## 2. Regeln speichern

Eine Regel hat Muster-Typ (Glob, Regex, Exakt), Muster, Aktion (Allow,
Confirm, Deny), Geltungsbereich (Global, Server, Tag) und Priorität (ganze
Zahl, Standard 0). Anlegen, Ändern und Löschen sind möglich. Beim Auflisten
lässt sich nach Geltungsbereich filtern (Global, ein Server, ein Tag oder
alle).

Muster werden beim Speichern geprüft (Spec 0077): Eine Regel mit einem Muster,
das sich nicht übersetzen lässt, wird nicht gespeichert.

## 3. Hard-Blacklist und bekannte Tags

- Die Hard-Blacklist (Spec 0002, Abschnitt 3.1) ist nur lesbar abrufbar.
  Sie lässt sich weder ändern noch löschen.
- Für die Auswahl des Geltungsbereichs steht die Liste der bereits
  verwendeten Tags zur Verfügung.

## 4. Erklärte Auswertung

Neben der eigentlichen Entscheidung kann die Engine zu einem
Beispielkommando eine nachvollziehbare Spur liefern:

- die Entscheidung,
- die gegriffene Regel (leer, wenn Hard-Blacklist oder Standard gegriffen
  hat) samt deren Herkunft (Spec 0037),
- den gegriffenen Hard-Blacklist-Eintrag,
- bei verketteten Kommandos je Teilkommando eine eigene Spur (Spec 0002,
  Abschnitt 4).

Die erklärte Auswertung dient ausschließlich dem Testen-Panel (Abschnitt 6).
Die Kommandoschleife benutzt die normale Auswertung, damit sie nicht
komplizierter wird.

## 5. Anbindung an die Kommandoschleife

Die Kommandoschleife wertet jeden Vorschlag gegen die gespeicherten Regeln
aus. Ablauf und Entscheidungslogik sind dieselben wie ohne Regeln; es ändert
sich nur, dass Nutzerregeln greifen.

## 6. Oberfläche: Regel-Manager

- **Regel-Liste**, gruppiert nach Geltungsbereich: zuerst Global, dann je
  Server, dann je Tag; innerhalb einer Gruppe nach Priorität. Jede Zeile zeigt
  das Muster, die Aktion (Allow grün, Confirm gelb, Deny rot) und die
  Priorität. Die Priorität lässt sich mit Auf-/Ab-Schaltflächen ändern, nicht
  per Drag-and-Drop (wie in Spec 0008). Eine Regel mit ungültigem Muster ist
  sichtbar markiert, und ihre Pfeile sind deaktiviert (Spec 0077).
- **Hard-Blacklist-Bereich**, deutlich als „fest codiert, nicht bearbeitbar"
  gekennzeichnet. Er zeigt die Muster aus Spec 0002, Abschnitt 3.1, damit der
  Nutzer weiß, dass sie immer greifen.
- **Regel-Formular:** Muster-Typ mit Eingabefeld, Aktion, Geltungsbereich
  (Global, ein Server aus der Serverliste oder ein Tag; ein neues, noch nicht
  verwendetes Tag lässt sich eintippen) und Priorität als Zahlenfeld.
- **Testen-Panel:** Eingabefeld für ein Beispielkommando, optional ein
  simulierter Server und simulierte Tags (Tags eines gewählten Servers werden
  übernommen). „Testen" zeigt die Gesamtentscheidung und bei verketteten
  Kommandos jedes Teilkommando mit seiner eigenen Entscheidung sowie die
  gegriffene Regel bzw. den Hard-Blacklist-Eintrag. So wird die Logik „das
  strengste Teilergebnis gewinnt" sichtbar.

## 7. Grenzen

- Regeln lassen sich nicht deaktivieren, ohne sie zu löschen.
- Ein Import oder Export von Regelsätzen ist nicht vorgesehen.
