# Spec 0110 — Dateianhänge im KI-Chat

Status: Kern umgesetzt (Nachrichtenmodell, Grenzen, Fencing, Redaction,
Speicherung, Auflöser-Erweiterungspunkt) und der Erweiterungspunkt im
Eingabefeld (Anhang-Chips, Ablegen). Die Auswahl von Dateien liefert eine
Edition (eigene Issues); die Community Edition registriert weder Auflöser
noch Eingabefeld-Aktion und verhält sich unverändert. Das Anhangsmodell im Kern (Nachrichtenvariante,
Grenzen, Fencing, Redaction, Speicherung, Auflöser- und Eingabefeld-
Erweiterungspunkt) ist generische Infrastruktur, die jede Edition nutzen
kann; der Kern wählt selbst keine Dateien aus.
Zweck: Eine Nutzer-Nachricht kann Dateien (Text, Bild, PDF) mitführen. Sie
werden mit der Sitzung gespeichert und sicher an die KI gesendet.
Bezüge: Spec 0006 (Redaction), Spec 0034/0040 (Speicherung), Spec 0037
(Berechtigungen), Spec 0039 (nicht vertrauenswürdige Inhalte).

## 1. Anhänge

- Ein Anhang hat Name, Medientyp, Art (Text, Bild, PDF), Inhalt und Größe.
  Text ist UTF-8; ein „Text"-Anhang mit ungültigem UTF-8 wird abgelehnt.
- Anhänge gehören zu einer Nutzer-Nachricht und stehen im Verlauf direkt
  nach deren Text.
- Der Kern wählt selbst keine Dateien aus. Eine Edition liefert sie über
  einen **Auflöser**, der eine undurchsichtige Anhang-ID einer Sitzung in
  einen Anhang übersetzt. Pfade aus dem Webview werden nie angenommen.
- Die Community Edition registriert keinen Auflöser.

## 2. Grenzen

- Höchstens 5 Anhänge je Nachricht, höchstens 1 MiB je Anhang (gemessen an
  den tatsächlichen Bytes, nicht an einer mitgelieferten Größenangabe).
- Die Grenzen gelten im Kern, unabhängig davon, woher die Anhänge stammen.
  Eine Verletzung schlägt geschlossen fehl: Die Nachricht wird nicht
  gesendet, es erfolgt kein Aufruf an die KI, der Nutzer erhält einen
  Fehler mit stabilem Code.

## 3. Senden einer Nachricht mit Anhängen

- `send_chat_message` nimmt optional eine Liste von Anhang-IDs.
  Ohne IDs verhält sich das Senden exakt wie ohne dieses Feature.
- Mit IDs und ohne registrierten Auflöser: Fehler, die Nachricht wird nicht
  gesendet. Eine unbekannte ID führt ebenfalls zu einem Fehler.
- Läuft gerade ein KI-Turn der Sitzung, wird eine Nachricht mit Anhängen
  abgelehnt (nur Text wird eingereiht); Anhänge gehen nie still verloren.

## 4. Nicht vertrauenswürdig, redigiert, gefencet

- Anhänge zählen als nicht vertrauenswürdige Quelle (Spec 0039, Abschnitt
  1, „lokale Datei"). Nach einer Nachricht mit Anhang gilt die Eskalation
  aus Spec 0039, Abschnitt 5 wie nach Remote-Dateiinhalt, auch nach dem
  Fortsetzen einer gespeicherten Sitzung.
- Textanhänge werden redigiert und danach als `local_file` gefencet, mit dem
  Dateinamen als Quelle; Fence-Marker im Inhalt oder Namen werden
  neutralisiert. Das geschieht bei jedem Senden erneut, auch nach dem
  Fortsetzen.
- Bilder und PDFs werden nie als Anweisung interpretiert. Sie gehen
  unverändert (nicht redigiert) als eigene Bild- bzw. Dokumentblöcke des
  Anbieters an die KI (Spec 0006, Abschnitt 8). Direkt davor steht ein
  gefencter `local_file`-Text mit dem Dateinamen, der sie als nicht
  vertrauenswürdige lokale Datei kennzeichnet.
- Kann das gewählte Modell Bilder bzw. PDFs nicht lesen, wird die Nachricht
  mit einem Fehler abgelehnt („Dieses Modell kann das angehängte Bild oder
  PDF nicht lesen"), bevor eine Anfrage an den Anbieter geht. Bilder und
  PDFs werden nie stillschweigend weggelassen; Textanhänge funktionieren
  immer. Der Fehler gilt, solange ein solcher Anhang im Verlauf steht;
  Abhilfe ist ein anderes Modell oder ein neuer Chat.

## 5. Speicherung

- Anhänge werden mit der Nachricht gespeichert (redigiert, wie Spec 0034,
  Abschnitt 3) und beim Fortsetzen geladen. Sitzungen ohne Anhänge laden
  unverändert.
- Löscht man eine Sitzung (oder den Server), verschwinden ihre Anhänge mit.
- Nach dem Fortsetzen zeigt der Verlauf Name und Art (Text, Bild, PDF) der
  Anhänge als Zeile; Inhalte erscheinen dort nie.

## 6. Kontextbegrenzung

- Die Token-Schätzung zählt Textanhänge nach Länge, Bilder und PDFs mit
  einem festen, konservativen Wert je Anhang. Dieselbe Schätzung gilt für
  die Ratenbegrenzung (Spec 0051, Spec 0061) und die Kompaktierung.
- Die Kompaktierung darf einen Anhang wie andere Inhalte durch eine
  Zusammenfassung ersetzen.

## 7. Berechtigung

Die Funktion hat den Namen `chat_attachments` (Spec 0037, Abschnitt 2), ist
nicht dienst-gebunden und in der Ausgangsberechtigung „Free" gesperrt.

## 8. Sicherheitszusagen

- Ein Geheimnis in einem Textanhang erreicht den Anbieter nie im Klartext.
- Ein Anhang kann seinen Fence nicht schließen.
- Es gibt keinen Weg, einen Anhang an der Redaction oder am Fencing
  vorbei zu senden.

## 9. Akzeptanzfälle

1. Textanhang mit bekanntem Geheimnismuster: das Muster fehlt in der
   Anfrage an den Anbieter, auch wenn der Anhang unredigiert aus der
   Speicherung kam.
2. Sechs Anhänge oder einer über 1 MiB: Fehler, kein Anbieteraufruf.
3. IDs ohne Auflöser: Fehler; ohne IDs: unverändert.
4. Vor dieser Änderung gespeicherte Sitzung: lädt und setzt unverändert fort.
5. Sitzung löschen entfernt die Anhänge aus der Datenbank.
6. Nach einem Anhang wird ein von der KI vorgeschlagenes Kommando gemäß
   Spec 0039, Abschnitt 5 eskaliert.
7. Bild und PDF an Claude, OpenAI (Responses) und einen OpenAI-kompatiblen
   Anbieter: die Anfrage enthält je einen Bild- und einen Dokumentblock im
   jeweiligen Anbieterformat.
8. Bild an ein reines Textmodell (z. B. lokales Ollama-Modell): Fehler, kein
   Anbieteraufruf; derselbe Chat mit nur einem Textanhang geht durch.
9. Geheimnismuster in einem Textanhang derselben Nachricht wie ein Bild: das
   Muster fehlt in der Anfrage, die Bildbytes sind unverändert.

## 10. Eingabefeld

- Registrierte Aktionen erscheinen als Symbol-Schaltflächen neben „Senden“
  (Spec 0038, §4). Ohne Aktion sieht das Eingabefeld aus wie zuvor.
- Ausstehende Anhänge stehen als Chips über dem Textfeld: Name, Größe und
  eine Schaltfläche zum Entfernen mit zugänglicher Beschriftung. Ein Chip,
  den die Edition als „unredigiert“ markiert, zeigt sichtbar den Hinweis
  „unredigiert gesendet“ (in allen UI-Sprachen).
- Beim Senden gehen die IDs der Chips mit; danach sind die Chips leer. Die
  gesendete Nachricht zeigt ihre Anhänge (Name, Art).
- Lehnt das Backend die Nachricht ab (Grenze, Modell ohne Unterstützung,
  gesperrte Funktion), steht die Meldung am Eingabefeld; Text und Chips
  bleiben zum Korrigieren erhalten, nichts wurde gesendet. Eine gesperrte
  Funktion öffnet zusätzlich den zentralen Hinweis-Dialog (Spec 0038, §4).
- Dateien, die auf den Chat gezogen werden, gehen an den registrierten
  Ablege-Handler; die Pfade verlassen das Backend nicht (ADR 0113). Ohne
  Handler passiert beim Ablegen im Chat nichts.
