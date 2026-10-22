# Spec 0086 — Dateibrowser: Größengrenzen, Transfer-Meldung beim Widerruf, Transport-Kanal-Paarung

Status: umgesetzt
Zweck: Die Größengrenze von „Dateiinhalt kopieren“ hält auch, wenn die Datei zwischen Prüfung und Lesen wächst, und „Lokal öffnen“ hat eine eigene Grenze. Die Übertragungsliste zeigt beim Widerruf denselben Text wie der Befehl. Der Transport einer Sitzung lässt sich von außen nicht getrennt von ihrem normalen SFTP-Kanal austauschen.
Bezüge: Spec 0020 (Dateibrowser, KI-Lesegrenze), Spec 0054 (Aktionen), Spec 0067 (erhöhter Modus), Spec 0085 (Widerruf, normaler Kanal), ADR 0078, ADR 0080.
Review-Priorität: erhöht für A3 (Sitzungsaufbau, normaler Kanal), sonst normal.

## 1. Verhalten im Überblick

- „Dateiinhalt kopieren“ funktioniert für Textdateien bis 50 MB und lehnt
  eine größere Datei ab, auch wenn sie erst nach der Größenabfrage so groß
  wurde. Ab 5 MB zeigt die Oberfläche Fortschritt und einen Abbrechen-Knopf. „Lokal öffnen“ lehnt Dateien über
  50 MB ab, vor dem Lesen, wenn die Größe schon bekannt ist, sonst nach dem
  Lesen.
- Scheitert ein Download oder Upload über den erhöhten Kanal, weil der Modus
  widerrufen wurde, nennt die Übertragungsliste denselben Text wie der
  Befehl.
- Transport und normaler SFTP-Kanal einer Sitzung bleiben ein Paar: Außerhalb
  der Anwendungslogik lässt sich der Transport weder ersetzen noch
  herausnehmen noch gegen den einer anderen Sitzung tauschen.

## 2. Nicht-Ziele

- Kein begrenztes Lesen für „Lokal öffnen“ und die Vorschau des
  Upload-Vergleichs: Eine Datei darf dort kurz ganz im Speicher liegen, sie
  wird nur nicht weitergegeben. „Dateiinhalt kopieren“ liest dagegen
  begrenzt (A1.1).
- Keine Grenze für Download, Upload oder die KI-Lesepfade (für die KI gilt
  Spec 0020, Abschnitt 4.1).
- Die Meldung bei einem Wechsel des Zielnutzers im erhöhten Modus bleibt, wie
  sie ist.

## 3. Anforderungen

### A1 — Größengrenzen

- **A1.1** „Dateiinhalt kopieren“ kopiert Textdateien (gültiges UTF-8) bis
  einschließlich 50 MB (50 × 1024 × 1024 Bytes); die Grenze ist fest. Eine
  größere Datei wird abgelehnt, mit der übersetzten Meldung „Die Datei ist
  größer als 50 MB und kann nicht in die Zwischenablage kopiert werden.“
  (Fehlercode, kein deutscher Rohtext aus dem Backend). Das gilt auch, wenn
  die Größenabfrage vorher bestanden hat oder gescheitert ist: Gelesen
  werden höchstens 50 MB + 1 Byte, danach wird abgebrochen; der Speicher hält
  für diese Aktion nie mehr. Eine Nicht-Text-Datei zeigt die bestehende
  Meldung „keine Textdatei“. Lehnt die Zwischenablage des Betriebssystems den
  Inhalt ab, zeigt die bestehende Meldung „Kopieren fehlgeschlagen“ den
  Grund.
- **A1.2** „Lokal öffnen“ lehnt eine Datei über 50 MB (50 × 1024 × 1024
  Bytes) ab. Meldung, wörtlich: `Datei ist größer als 50 MB — zu groß zum
  lokalen Öffnen. Bitte stattdessen herunterladen.` Ein gescheiterter
  Größenabruf bricht den Befehl wie bisher ab.
- **A1.3** Bei einer Ablehnung nach A1.2 entsteht **keine** lokale Datei und
  kein Ordner in der Bearbeitungskopie-Ablage. Eine dort schon liegende Kopie
  derselben Datei aus einem früheren „Lokal öffnen“ bleibt unverändert.
- **A1.4** Beide Grenzen gelten für den normalen und den erhöhten Kanal
  gleich. Die Grenze aus A1.2 ist unabhängig von der Grenze der Vorschau und
  des KI-Lesepfads: Eine manuelle Aktion läuft nie über Code der
  KI-Infrastruktur (Redaktion, Filter-Abbildung); gleiche Zahlenwerte sind
  Zufall, keine geteilte Definition (Spec 0054, Sicherheitsmodell).

- **A1.5** Ab 5 MB (bekannte Dateigröße, sonst gelesene Bytes) zeigt die
  Oberfläche „Kopiere… n MB von m MB“ und einen Abbrechen-Knopf; sie bleibt
  bedienbar. Abbrechen beendet das Lesen, kopiert nichts und zeigt keine
  Fehlermeldung.
- **A1.6** Die Grenze der Vorschau im Upload-Vergleich (256 KB) und der
  KI-Lesepfad (Spec 0020, Abschnitt 4.1) bleiben unverändert.

### A2 — Transfer-Meldung beim Widerruf

- **A2.1** Scheitert ein Download oder Upload über den erhöhten Kanal, weil
  der Modus widerrufen wurde, trägt das Fehlerfeld der Meldung
  „Übertragung beendet“ wörtlich denselben Text wie das Befehlsergebnis
  (`ELEVATED_CHANNEL_INACTIVE`), ohne Präfix.
- **A2.2** Für alle anderen Fehler bleibt der Text, wie er ist. Eine
  Übertragung, die vor dem Widerruf fertig war, meldet weiter Erfolg.

### A5 — Übertragungen in Stücken, Fortschritt, Abbrechen

- **A5.1** Up- und Downloads laufen in Stücken von höchstens 256 KiB; der
  Speicher hält je Übertragung höchstens ein Stück, nie die ganze Datei. Die
  Größengrenzen aus A1 bleiben unverändert.
- **A5.2** Ein Download wird in eine Temp-Datei neben dem Ziel geschrieben und
  erst nach vollständigem Erfolg unter dem Zielnamen abgelegt; eine vorhandene
  Datei gleichen Namens wird erst dann ersetzt. Bei Fehler oder Abbruch ist die
  Temp-Datei weg und die vorhandene Datei unverändert. Im erhöhten Modus ist
  die lokale Datei nur für den eigenen Nutzer lesbar.
- **A5.3** Jede Übertragung meldet Fortschritt (übertragene Bytes, Gesamtgröße,
  falls bekannt), höchstens ca. viermal pro Sekunde, nie rückwärts. Die Zeile
  zeigt einen Fortschrittsbalken, solange die Gesamtgröße bekannt ist.
  Ordner-Übertragungen melden zusätzlich Dateien fertig / gesamt und Bytes
  fertig / gesamt, sobald der Baum durchlaufen ist.
- **A5.4** Jede laufende Übertragung hat einen Abbrechen-Knopf. Abbrechen
  beendet sie nach dem laufenden Stück (bei einem Ordner den ganzen Ordner); die
  Zeile zeigt „abgebrochen“, keine Fehlermeldung, kein Fehler-Toast. Eine
  Teildatei lokal wird gelöscht; eine Teildatei auf dem Server wird entfernt,
  gelingt das nicht, nennt die Zeile den unvollständigen Pfad. Dasselbe gilt
  mit „gestoppt“, wenn der Nutzer das Beenden des erhöhten Modus bestätigt
  (Spec 0085 A1.3).

### A3 — Transport und Kanal bleiben ein Paar

- **A3.1** Code außerhalb der Anwendungslogik darf den Transport einer
  bestehenden Sitzung weder ersetzen noch herausnehmen noch mit dem einer
  anderen Sitzung tauschen. Das gilt für das Feld selbst, für jeden Weg, der
  es mitnimmt (etwa ein Tausch oder Ersetzen aller Bestandteile einer
  Sitzung), und für die Sperre des Transports, die schon mit einer
  gemeinsamen Referenz auf die Sitzung erreichbar ist. Nachweis beim
  Übersetzen, wie in Spec 0085 T11.
- **A3.2** Erlaubt bleiben: der Tausch zweier **ganzer** Sitzungen (Transport
  und Kanal wandern zusammen), das Sperren des Transports **zum Benutzen**
  (alle Methoden des Transports) und das Bauen einer Sitzung aus allen
  Bestandteilen. Die Sperre gibt keine Referenz heraus, über die sich der
  Transport selbst ersetzen ließe.
- **A3.3** Tests können Sitzungen mit gezielt gesetzten Feldern bauen, aber
  nur über eine Testhilfe, die in Produktivbauten nicht enthalten ist.
- **A3.4** Kein Verhaltensunterschied im Betrieb. Die Zusagen aus Spec 0085
  A3 gelten unverändert; ihre verbotenen Fälle bleiben Übersetzungsfehler.

### A4 — Verweise in Kommentaren

Entfallen als Anforderung: eine einmalige Aufräumaufgabe, erledigt.
**A4.2:** Ein Verweis, dessen Ziel es nirgends mehr gab, wurde auf das
heutige Ziel umgeschrieben oder entfernt; die Kennung bleibt vergeben.

## 4. Sicherheitszusagen

- **KI und MCP nie über den erhöhten Kanal** (Spec 0067 A): A3 verstärkt das,
  weil der normale Kanal nur noch zum Transport seiner eigenen Sitzung
  gehört. Keine Anforderung lockert Spec 0085 A3.
- **Widerruf** (Spec 0085 A1): A2 ändert nur den Text in der Meldung; die
  maßgebliche Widerrufsprüfung unter der Sperre bleibt unberührt.
- **Bearbeitungskopien** (Spec 0054, Teil 4; Spec 0067 A5): Eine Ablehnung
  schreibt nichts. Im Erfolgsfall bleiben die Rechte unverändert (nur für den
  Nutzer lesbar).
- Keine neue Datensenke.

## 5. Abnahmefälle

- **T1 (A1.1)** Eine 10-MB-Datei wird vollständig kopiert.
- **T2 (A1.1)** `stat` meldet 100 Bytes, gelesen werden 50 MB + 1 Byte:
  Ablehnung mit dem Code der Größenablehnung; bei `stat` > 50 MB wird gar
  nicht gelesen.
- **T3 (A1.1)** Genau 50 MB gültiges UTF-8 werden zurückgegeben (Grenzwert),
  50 MB + 1 Byte nicht; beides auch über den erhöhten Kanal.
- **T3a (A1.1)** `stat` scheitert, die Datei hat 60 MB: Ablehnung nach
  höchstens 50 MB + 1 gelesenen Bytes.
- **T3b (A1.5)** Eine 6-MB-Datei zeigt Fortschritt; Abbrechen beendet das
  Lesen und kopiert nichts.
- **T3c (A1.6)** Die Vorschau des Upload-Vergleichs lehnt weiter über 256 KB
  ab.
- **T4 (A1.2)** `stat` meldet 50 MB + 1: Ablehnung mit dem Text aus A1.2,
  ohne dass gelesen wurde; kein Eintrag in der Bearbeitungskopie-Ablage.
- **T5 (A1.2/A1.3)** `stat` meldet 10 Bytes, gelesen werden 50 MB + 1:
  Ablehnung, keine lokale Datei; eine vorhandene Kopie ist danach
  byte-gleich.
- **T6 (A1.2)** Genau 50 MB werden geöffnet (Grenzwert).
- **T7 (A1.4)** T3 und T4 auch über den erhöhten Kanal.
- **T8 (A2.1)** Download und Upload über den erhöhten Kanal, Widerruf nach
  dem Erstzugang: Das Ereignis „Übertragung beendet“ hat denselben Fehlertext
  wie das Befehlsergebnis, `ELEVATED_CHANNEL_INACTIVE`.
- **T9 (A2.2)** Ein Download, der mit einem gewöhnlichen Kanalfehler
  scheitert (kein Widerruf), meldet weiter den Text mit Präfix.
- **T11 (A3.1)** Aus Sicht eines anderen Crates übersetzt nicht: (a) Tausch
  der Transporte zweier Sitzungen, (b) Ersetzen des Transports durch einen
  neuen Wert, (c) Tausch aller Bestandteile zweier Sitzungen, (d) Zuweisung
  eines neuen Transports, (e) mit nur einer gemeinsamen Referenz: Zuweisung
  oder Tausch über die Sperre. Übersetzt: Tausch zweier ganzer Sitzungen und
  ein Methodenaufruf über die Sperre; jeder verbotene Fall hat einen
  kompilierenden Zwilling, der sich nur in der verbotenen Zeile
  unterscheidet.
- **T12 (A3.3)** Bestehende Tests, die Felder nach dem Bau setzen, laufen in
  ihrer Aussage unverändert; kein Test wurde entfernt oder abgeschwächt.
