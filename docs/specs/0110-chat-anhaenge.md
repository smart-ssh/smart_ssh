# Spec 0110 — Erweiterungsinhalte in Chat-Nachrichten

Status: umgesetzt (Verweis, Grenzen, Versiegeln, Speicherung) und der
Erweiterungspunkt im Eingabefeld (Chips, Ablegen). Der Kern hat kein eigenes
Anhangsmodell: Inhalt an Chat-Nachrichten gibt es nur über Erweiterungen
(Spec 0038, Abschnitt 2a; ADR 0141). Der Kern wählt selbst keine Dateien aus.
Die Community Edition registriert keine Erweiterung und keine
Eingabefeld-Aktion und verhält sich unverändert.
Zweck: Eine Nutzer-Nachricht kann Inhalt (Text, Bild, PDF) einer Erweiterung
mitführen. Er wird sicher an die KI gesendet; im Chatverlauf steht nur ein
Verweis.
Bezüge: Spec 0006 (Redaction), Spec 0034/0040 (Speicherung), Spec 0037
(Berechtigungen), Spec 0038 (Erweiterungen), Spec 0039 (nicht
vertrauenswürdige Inhalte).

## 1. Erweiterungsinhalt (Verweis statt Inhalt)

Eine Edition kann eigenen Inhalt an eine Nutzer-Nachricht hängen, ohne dass
der Kern dessen Datenmodell kennt.

- Im Chatverlauf steht nur ein **Verweis**: Name der Erweiterung (vom Kern
  gesetzt), eine für den Kern undurchsichtige Art und ID, ein redigierter
  Anzeigename, die Medienklasse (Text, Bild, PDF) und die Token-Schätzung.
  Der Inhalt selbst liegt nie im Chatverlauf. Je Verweis gibt es einen
  eigenen Eintrag direkt nach dem Text der Nachricht.
- Der Kern wählt keine Dateien aus. Eine Erweiterung übersetzt eine
  undurchsichtige ID einer Sitzung in Inhalt. Pfade aus dem Webview werden
  nie angenommen.
- Ein Verweis, dessen Inhalt nicht versiegelt zum Anbieter geht, erscheint
  dort nur als feste, als nicht vertrauenswürdig gekennzeichnete Zeile
  „Inhalt nicht mehr verfügbar“ mit dem redigierten Anzeigenamen; die
  Anfrageprotokolle zeigen nur Anzahlen.
- Ein Verweis zählt als nicht vertrauenswürdiger Inhalt (Spec 0039,
  Abschnitt 1 „lokale Datei“ und Abschnitt 5), auch nach dem Fortsetzen
  einer gespeicherten Sitzung.

## 2. Senden einer Nachricht mit IDs

- `send_chat_message` nimmt optional eine Liste von IDs. Ohne IDs verhält
  sich das Senden exakt wie ohne dieses Feature.
- Bei einer Nachricht mit IDs fragt der Kern die registrierten Erweiterungen
  der Reihe nach, bis eine die ID übersetzt. Eine unbekannte ID führt zum
  Fehler `ATTACHMENT_UNRESOLVED`; IDs ohne registrierte Erweiterung zum
  Fehler `ATTACHMENTS_UNAVAILABLE`. In beiden Fällen wird nichts gesendet.
- Vor dem Speichern prüft der Kern die allgemeinen Grenzen, leitet die
  Medienklasse ab, prüft Binärtypen, redigiert Text und Anzeigenamen und
  berechnet die Token-Schätzung. Erst dann übergibt er die geprüften Blöcke
  der Erweiterung zum Speichern. Scheitert das Speichern, wird nichts
  gesendet.
- Läuft gerade ein KI-Turn der Sitzung, wird eine Nachricht mit IDs
  abgelehnt (`ATTACHMENTS_WHILE_BUSY`) und nichts gespeichert; nur Text wird
  eingereiht. Inhalt geht nie still verloren.

## 3. Allgemeine harte Grenzen

- Höchstens 5 Teile je Nachricht, höchstens 4 Blöcke je Teil, höchstens
  1 MiB je Block (tatsächliche Bytes), Text ist gültiges UTF-8, und
  höchstens 20 MiB Binärbytes je Anfrage über den ganzen Verlauf.
- Geprüft wird beim Annehmen und bei jedem Senden, unabhängig davon, woher
  der Inhalt stammt. Ein Verstoß schlägt geschlossen fehl: Es wird nichts
  gesendet, es erfolgt kein Aufruf an die KI, der Nutzer erhält einen
  Fehler mit stabilem Code: `ATTACHMENTS_TOO_MANY` (Teile oder Blöcke),
  `ATTACHMENT_TOO_LARGE`, `ATTACHMENT_INVALID`,
  `ATTACHMENTS_REQUEST_TOO_LARGE` (20 MiB je Anfrage).
- Erlaubte Binärtypen: `image/png`, `image/jpeg`, `image/gif`, `image/webp`,
  `application/pdf`; die ersten Bytes müssen zum gemeldeten Typ passen.
  Alles andere wird vor jedem Anbieteraufruf abgelehnt. Binärbytes werden nie
  als Text gelesen, redigiert oder protokolliert.

## 4. Versiegeln beim Senden

- Bei jedem Senden an einen Anbieter (Chat-Turn und Kompaktierung) löst der
  Kern jeden Verweis über seine Erweiterung auf und macht daraus einen
  versiegelten Inhalt: Text wird erneut redigiert (auch wenn die Erweiterung
  unredigierten Text liefert), als `local_file` mit dem Anzeigenamen als
  Quelle gefencet (Fence-Marker werden neutralisiert) und von einer festen
  Sicherheitszeile gefolgt; Binärblöcke gehen unverändert als Bild- bzw.
  Dokumentblock des Anbieters (Spec 0006, Abschnitt 8), davor eine gefencte
  Kennzeile mit dem Dateinamen. Text einer Erweiterung steht nie außerhalb
  des Fence. Bilder und PDFs werden nie als Anweisung interpretiert.
- Liefert die Erweiterung „Inhalt existiert nicht mehr“ oder ist sie nicht
  registriert, geht die feste Zeile „Inhalt nicht mehr verfügbar“ mit dem
  Anzeigenamen. Liefert sie einen Fehler, wird nichts gesendet.
- Versiegelter Inhalt wird nie gespeichert.
- Kann das gewählte Modell Bilder bzw. PDFs nicht lesen, wird die Nachricht
  mit einem Fehler abgelehnt („Dieses Modell kann das angehängte Bild oder
  PDF nicht lesen“), bevor eine Anfrage an den Anbieter geht. Bilder und
  PDFs werden nie stillschweigend weggelassen; Text funktioniert immer. Der
  Fehler gilt, solange ein solcher Inhalt im Verlauf steht; Abhilfe ist ein
  anderes Modell oder ein neuer Chat.

## 5. Speicherung und Verlauf

- Gespeichert wird nur der Verweis mit der Nachricht (wie Spec 0034,
  Abschnitt 3) und beim Fortsetzen geladen. Sitzungen ohne Verweise laden
  unverändert.
- Die Erweiterung verwaltet den Inhalt unter der ID der gespeicherten
  Chat-Sitzung. Deshalb bleibt er nach einem Neustart und dem Fortsetzen
  der Sitzung verfügbar; nach „Neuer Chat“ gehört neuer Inhalt zum neuen Chat.
- Löscht man eine Sitzung (oder den Server), verschwinden ihre Verweise mit;
  die Erweiterung wird darüber informiert und räumt ihren Inhalt auf.
- Nach dem Fortsetzen zeigt der Verlauf Anzeigename und Art (Text, Bild, PDF)
  als Zeile, nie Inhalt, und ohne die Erweiterung zu fragen.
- Ein früherer Entwicklungsstand speicherte Anhänge als eigene Nachrichtenart
  im Chatverlauf. Diese Einträge wurden entfernt; alle übrigen Nachrichten
  einer solchen Sitzung laden unverändert.

## 6. Kontextbegrenzung

- Die Token-Schätzung des Verweises (vom Kern beim Annehmen berechnet) und
  der versiegelte Inhalt zählen für die Ratenbegrenzung (Spec 0051,
  Spec 0061) und die Kompaktierung; Binärblöcke mit einem festen,
  konservativen Wert.
- Die Kompaktierung darf einen Verweis wie andere Inhalte durch eine
  Zusammenfassung ersetzen.

## 7. Berechtigung

Die Funktion hat den Namen `chat_attachments` (Spec 0037, Abschnitt 2), ist
nicht dienst-gebunden und in der Ausgangsberechtigung „Free“ gesperrt.

## 8. Sicherheitszusagen

- Ein Geheimnis in Erweiterungsinhalt oder Anzeigename erreicht den Anbieter
  nie im Klartext.
- Erweiterungsinhalt kann seinen Fence nicht schließen.
- Es gibt keinen Weg, Erweiterungsinhalt an der Redaction oder am Fencing
  vorbei zu senden: Eine Erweiterung kann keinen versiegelten Inhalt selbst
  bauen.

## 9. Akzeptanzfälle

1. Geheimnis in Text und Anzeigename fehlt in der Anfrage, auch wenn die
   Erweiterung unredigiert liefert und auch nach dem Fortsetzen; ein
   Fence-Marker im Text oder Namen schließt den Fence nicht.
2. Sechs Teile, fünf Blöcke, ein Block über 1 MiB, ein Binärblock, dessen
   Bytes nicht zum Typ passen, ein Typ außerhalb der Liste, oder mehr als
   20 MiB Binärbytes in einer Anfrage: Fehler mit stabilem Code, kein
   Anbieteraufruf; die genauen Grenzen gehen durch.
3. IDs ohne Erweiterung: Fehler; ohne IDs: unverändert.
4. Fehlender Inhalt oder fehlende Erweiterung: feste Platzhalterzeile; Fehler
   beim Auflösen: nichts wird gesendet.
5. Vor dieser Änderung gespeicherte Sitzung: lädt und setzt unverändert fort;
   eine Entwicklungsdatenbank mit alten Anhang-Einträgen öffnet, die Einträge
   sind weg, die übrigen Nachrichten laden.
6. Sitzung löschen entfernt die Verweise aus der Datenbank und informiert die
   Erweiterung.
7. Nach einem Verweis wird ein von der KI vorgeschlagenes Kommando gemäß
   Spec 0039, Abschnitt 5 eskaliert.
8. Bild und PDF an Claude, OpenAI (Responses) und einen OpenAI-kompatiblen
   Anbieter: die Anfrage enthält je einen Bild- und einen Dokumentblock im
   jeweiligen Anbieterformat.
9. Bild an ein reines Textmodell (z. B. lokales Ollama-Modell): Fehler, kein
   Anbieteraufruf; derselbe Chat mit nur Text geht durch.
10. Geheimnis in Text derselben Nachricht wie ein Bild: das Muster fehlt in
    der Anfrage, die Bildbytes sind unverändert.

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
- Ein Ablegen geht nur an den Chat, wenn der Zeiger beim Ablegen über dem
  sichtbaren Chat-Bereich liegt; ein Ablegen über dem Dateibrowser oder einer
  anderen Fläche erreicht die Handler nicht. Solange mindestens ein Handler
  registriert ist und eine Datei über den Chat gezogen wird, zeigt der Chat
  eine Ablege-Markierung. Sie verschwindet, wenn der Zeiger den Chat
  verlässt, beim Abbruch und beim Ablegen. Ohne Handler gibt es keine
  Markierung.
- Ein Handler lässt sich über `unregisterComposerDropHandler(id)` wieder
  entfernen; Abonnenten werden benachrichtigt. Ist der letzte entfernt,
  nimmt der Chat keine Drops mehr an und zeigt keine Markierung. Eine
  unbekannte `id` bewirkt nichts.
