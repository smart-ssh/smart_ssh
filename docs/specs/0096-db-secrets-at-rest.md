# Spec 0096 — Geheimnisse in der Datenbank: Sitzungstitel schwärzen, Rohdatei-Nachweis

Status: umgesetzt
Zweck: Ein Kommando wie `mysql -p'geheim'` hinterlässt `geheim` nicht im Klartext in der Datenbankdatei — belegt durch einen Test, der die Datei selbst durchsucht.
Bezüge: Spec 0016 und 0095 (Redaction), Spec 0036 (Schutz der Chat-Inhalte), Spec 0101 (Datenbankverschlüsselung), Spec 0010 (Notizvorschlag beim Trennen), ADR 0088.
Review-Priorität: ERHÖHT (Redaction, Persistenz)

## 1. Was wo liegt

- **Verlauf** (Chat-Nachrichten, Ausführungsprotokoll, Eingabe-Historie,
  Zusammenfassungen) liegt unverändert in der Datenbank. Geschützt ist er
  durch die Verschlüsselung der ganzen Datenbankdatei (Spec 0101, Spec 0036
  Abschnitt 1); der Verlauf zeigt weiter, was der Nutzer getippt hat.
- **Klartext-Felder mit abgeleitetem Inhalt** werden vor dem Speichern
  geschwärzt: der von der KI erzeugte Sitzungstitel (A1).
- **Notizen** bleiben, wie der Nutzer sie schreibt. **Notizvorschläge der KI**
  werden geschwärzt, bevor der Nutzer sie im Vergleichsdialog sieht; was er
  bestätigt, ist genau das, was gespeichert wird (A2).
- Weitere Klartext-Felder der Datenbank sind Notizrevisionen, Notizen von
  Servern und Gruppen, Filterregelmuster, Sitzungstitel und Konfiguration
  der KI-Anbieter; sie liegen innerhalb der verschlüsselten Datei.

## 2. Entscheidungen

- **E1** Verschlüsselung ist der Schutz des Verlaufs; er wird nicht
  geschwärzt.
- **E2** Klartext-Stellen mit abgeleitetem Inhalt werden geschwärzt (Titel).
- **E3** Notizen des Nutzers bleiben unverändert; Notizvorschläge der KI
  werden vor der Anzeige geschwärzt.

## 3. Ziel und Nicht-Ziele

Ziel: Nach einer Sitzung, in der ein Geheimnis in Prompt, Kommando, Ausgabe,
KI-Antwort und Zusammenfassung vorkam, enthält keine Datei des
Datenbank-Verzeichnisses das Geheimnis im Klartext — außer in Notizen, die der
Nutzer selbst schreibt, und in Filterregeln aus dem Schnellvorschlag „Exakt".
Das gilt, soweit die Muster der Redaction greifen (K4).

Nicht-Ziele:

- Schwärzen **im** verschlüsselten Verlauf.
- Notizen des Nutzers schwärzen (E3).
- Manuell vergebene Sitzungstitel.
- Filterregeln aus dem Schnellvorschlag „Exakt": Die Regel braucht das
  Kommando wörtlich und entsteht durch eine bewusste Nutzerhandlung.
- Klartext-Reste alter Eingabe-Historie in freien Seiten einer
  Bestands-Datenbank aus der Zeit vor der Verschlüsselung.
- Neue Redaction-Muster (Spec 0095).

## 4. Anforderungen

**A1 Titel geschwärzt.** Ein von der KI erzeugter Sitzungstitel läuft vor dem
Speichern durch die Redaction der Sitzung, und zwar **vor** dem Kürzen auf 60
Zeichen. Enthält er danach nur noch Platzhalter und Leerraum, wird kein Titel
gespeichert.

**A2 KI-Notizvorschläge geschwärzt.** Der Inhalt jedes Notizvorschlags der KI
läuft durch die Redaction der Sitzung, **bevor** er dem Nutzer angezeigt wird.
Erfasst sind die Wege, auf denen eine KI den Inhalt erzeugt: KI im Chat, beim
Trennen der Sitzung und ein externer Agent über MCP. **Nicht** erfasst ist „In
Notiz übernehmen": Dort wählt der Nutzer selbst eine Chatzeile aus (E3).
Angezeigt, bestätigt und gespeichert wird dieselbe geschwärzte Fassung. Ist sie
leer, entsteht kein Vorschlag. Bewusst hingenommen: Enthält die bestehende
Notiz ein vom Nutzer selbst geschriebenes Muster und übernimmt die KI es in
ihren Vorschlag, zeigt der Vergleichsdialog es als `[REDACTED]`; der Nutzer
sieht das vor dem Bestätigen.

**A3 Rohdatei-Nachweis.** Ein Test legt eine Datenbank in einem
Temp-Verzeichnis an, schreibt über die echten Stores je einen Eintrag mit einem
Geheimnis in: Chat-Nachricht, Kommando-Ergebnis (Kommando und Ausgabe),
Ausführungsprotokoll, Eingabe-Historie und Zusammenfassung. Danach durchsucht
er **jede Datei** des Verzeichnisses (Datenbank, `-wal`, `-shm`, `-journal`)
byteweise nach dem Geheimnis → kein Treffer. Die Suche läuft sowohl nach dem
Schließen der Verbindung als auch bei offener Datenbank, wo `-wal` und `-shm`
existieren.

**A4 Der Nachweis kann scheitern.** Derselbe Ablauf läuft ein zweites Mal gegen
eine eigene, unverschlüsselte Datenbank; dort muss dieselbe Suche das
Geheimnis **finden**. Findet sie es nicht, ist der Test rot.

## 5. Sicherheitszusagen

- **Redaction vor Datensenke:** verschärft um den Titel.
- **Verschlüsselung:** kein Pfad schreibt Klartext außerhalb der
  verschlüsselten Datenbankdatei.
- **Transparenz:** Chatverlauf und Eingabe-Historie zeigen weiter das Original;
  das Ausführungsprotokoll ist wie bisher redigiert.

## 6. Klarstellungen

- **K1** Der Titel-Test prüft neben dem Grenzfall (55 Zeichen Text, danach das
  Muster) zusätzlich 45 Zeichen und ein Muster mit nachlaufendem Anker
  (`https://u:<wert>@host`; die URL-Regel braucht das `@host` hinter dem Wert).
  Nur Letzteres trägt den Gegenbeweis dafür, dass Schwärzen vor dem Kürzen
  geschieht (ADR 0088, Abschnitt 4).
- **K2** Ein sauber geschlossener Verbindungspool hinterlässt keine
  `-wal`-Datei; deshalb gibt es zusätzlich die Suche bei offener Datenbank
  (A3, ADR 0088, Abschnitt 5).
- **K3** Ein vollständig geschwärzter Notizvorschlag wird im Chat und über
  MCP mit einer festen Meldung (ohne KI-Inhalt) angezeigt. Beim Trennen der
  Sitzung bleibt es bei einem Log-Eintrag, weil dort kommentarloses Beenden
  gilt (Spec 0010; ADR 0088, Abschnitt 2).
- **K4** Die Zusage aus Abschnitt 3 gilt, soweit die Muster der Redaction
  greifen; die Titel-Schwärzung kann vom Redactor nicht erkannten Inhalt nicht
  entfernen (ADR 0088, Abschnitt 6). Gesucht wird nur nach UTF-8-Bytes.

## 7. Testfälle

Geheimnis: `Geheim-0096`. Wo die Redaction beteiligt ist, in der Form
`password=Geheim-0096`; sie macht daraus `[REDACTED]`.

- **T1 Titel:** Die KI antwortet mit einem Titel, der `password=Geheim-0096`
  enthält → der gespeicherte Titel enthält das Geheimnis nicht.
- **T2 Titel nur Platzhalter:** Titel besteht nur aus `password=Geheim-0096` →
  kein Titel gespeichert.
- **T3 Rohdatei (A3):** kein Treffer in keiner Datei.
- **T4 Gegenprobe (A4):** dieselbe Suche findet das Geheimnis in einer
  unverschlüsselten Datenbank.
- **T5 Wächter:** Die Store-Tests zur Speicherform bleiben grün: ein neuer
  Eintrag steht als Klartext in der Datenbank; die Vertraulichkeit belegt T3.
- **T6 Kürzen (adversarial):** Titel mit 55 Zeichen Text und danach
  `password=Geheim-0096` → kein Bruchstück des Geheimnisses im Titel (K1).
- **T7 Mehrere Muster** im Titel, Geheimnis in Anführungszeichen → kein Treffer.
- **T8 Notizvorschlag im Chat (A2):** Ereignis an die Oberfläche und
  gespeicherte Revision nach Bestätigung enthalten das Geheimnis nicht.
- **T9 Notizvorschlag beim Trennen (A2):** dasselbe, geprüft werden Ereignis
  **und** Revision.
- **T10 Nur Muster (A2):** Vorschlag besteht nur aus dem Geheimnis → kein
  Vorschlag.
- **T11 Notizvorschlag über MCP (A2):** Ergebnis an den Client, Ereignis und
  Revision ohne Geheimnis.
- **T12 „In Notiz übernehmen" (A2, Gegenfall):** Die gewählte Chatzeile mit
  `password=Geheim-0096` steht unverändert in der Revision.
