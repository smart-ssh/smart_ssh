# Spec 0108 — Spracheingabe im KI-Chat

Status: umgesetzt (#416 Modellverwaltung, #417 Aufnahme und Erkennung,
#418 Oberfläche)
Zweck: Der Nutzer diktiert Text in das Eingabefeld des KI-Chats. Die
Erkennung läuft vollständig auf dem eigenen Rechner mit einem lokalen
Modell (Whisper oder Parakeet); Audio verlässt den Rechner nie.
Bezüge: docs/netzwerkverbindungen.md (jede Verbindung ist dort gelistet),
ADR 0086 (keine Inhalte in Standard-Logs).

## 1. Grundsatz

**1.1** Spracheingabe gehört zur Community-Edition.

**1.2** Die Erkennung läuft lokal, mit den Whisper-Modellen
(int8-ONNX-Export) oder mit dem Modell Parakeet TDT 0.6B v3, beide auf
derselben ONNX Runtime (nur CPU, fest in die App eingebunden; die Laufzeit
lädt nichts nach). Es gibt
keinen Cloud-Dienst für die Erkennung; Audio und erkannter Text werden nie
an einen Server geschickt.

**1.3** Die einzige Netzwerkverbindung dieser Funktion ist der Download des
Modells (Abschnitt 3). Sie ist in docs/netzwerkverbindungen.md aufgeführt.

## 2. Modelle

**2.1** Es gibt drei mehrsprachige Modelle: `base` (rund 74 MiB, Vorgabe),
`small` (rund 239 MiB) und `parakeet_v3` (Parakeet TDT 0.6B v3, int8, rund
640 MiB; auf allen Plattformen, auch Android). Kann die Engine das Modell
nicht laden (z. B. zu wenig Arbeitsspeicher, nicht unterstützte CPU), endet
die Aufnahme mit `speech_transcription_failed`, und die App bleibt
bedienbar. Ein Befehl liefert die auf der Plattform
verfügbaren Modelle mit Kennung, Engine (`whisper` oder `parakeet`),
Gesamtgröße in Bytes sowie Host und Repository der Quelle; er löst keine
Netzwerkanfrage aus.

**2.1a** Ein Modell kann aus mehreren Dateien bestehen (Parakeet: Encoder,
Decoder/Joint, Merkmals-Vorverarbeitung, Vokabular; Whisper: Encoder, Decoder, Vokabular, zusätzliche Token, Konfiguration).
Es ist nur „bereit“, wenn jede Datei geprüft wurde.

**2.2** Die Wahl des Modells ist eine Einstellung. Sie bleibt über Neustarts
erhalten, die Vorgabe ist `base`.

**2.3** Jedes Modell hat einen eigenen Zustand: nicht geladen, wird geladen
(mit Fortschritt in Bytes), bereit, Fehler. Wählt der Nutzer ein Modell, das
nicht geladen ist, ist dessen Zustand „nicht geladen“. Die Dateien der
anderen Modelle bleiben, bis der Nutzer sie löscht.

**2.4** Das Abfragen des Zustands und das Öffnen der Einstellungen lösen
keine Netzwerkanfrage aus.

## 3. Download

**3.1** Das Modell wird nur nach ausdrücklicher Handlung des Nutzers geladen
(Zustimmungsdialog, Abschnitt 5.5). Es gibt keinen Download beim Start und
keine Hintergrund-Aktualisierung.

**3.2** Quelle ist je Datei eines Modells eine feste URL auf einen
unveränderlichen Stand eines Repositories auf huggingface.co (kein wandernder
Branch): `onnx-community/whisper-base` bzw. `onnx-community/whisper-small` für Whisper, `istupakov/parakeet-tdt-0.6b-v3-onnx`
für Parakeet. Je Datei sind Größe und SHA-256-Prüfsumme fest hinterlegt.

**3.3** Der Download wird in temporäre Dateien im Datenverzeichnis der App
geschrieben (Unterordner `speech-models`; ein Modell aus mehreren Dateien
bekommt darin einen eigenen Ordner). Erst nachdem jede Datei von Größe und
SHA-256 her geprüft wurde, werden die Dateien unter ihren endgültigen Namen
abgelegt; keine Datei erscheint vorher unter ihrem endgültigen Namen. Stimmt
Größe oder Prüfsumme einer Datei nicht, bleibt nichts vom Modell übrig, der
Fehler `speech_model_hash_mismatch` wird gemeldet und nichts davon wird
geladen.

**3.4** Während des Downloads erhält die Oberfläche laufend Fortschritt
(geladene Bytes von insgesamt, bei mehreren Dateien die Summe über alle). Ein zweiter Download desselben Modells
parallel wird abgelehnt.

**3.5** Der Nutzer kann den Download abbrechen. Dann stoppt die Übertragung,
alle temporären und bereits fertig geladenen Dateien dieses Modells werden
entfernt und der Zustand ist „nicht geladen“. Auch ein fehlgeschlagener
Download hinterlässt kein Teilmodell.

**3.6** Fehler tragen einen übersetzbaren Schlüssel: Netzwerk, HTTP-Status,
Prüfsumme/Größe, Dateisystem. Der Fehlerzustand bleibt, bis der Nutzer
erneut lädt oder das Modell löscht.

**3.7** Der Nutzer kann ein Modell löschen. Alle Dateien des Modells
verschwinden (Dateien anderer Modelle bleiben), der
Zustand ist „nicht geladen“. Während eines laufenden Downloads ist Löschen
nicht möglich (erst abbrechen).

**3.8** Jeder Download eines Sprachmodells (auch Parakeet) erscheint als
Aktivität „Sprachmodell <Name>“ im Aktivitätszentrum (Spec 0116) mit
Fortschritt in Bytes, dem Status „Verifying“ während der Prüfsummenprüfung
und einer Abbrechen-Schaltfläche. Erfolg, Fehler und Abbruch beenden die
Aktivität entsprechend; bei einem Fehler zeigt die Zeile die übersetzte
Meldung zum Fehlerschlüssel. Der Download läuft weiter, wenn die
Spracheingabe-Einstellungen oder der Zustimmungsdialog geschlossen werden;
beide zeigen beim erneuten Öffnen den laufenden Download mit seinem
Fortschritt und starten keinen zweiten. Abbrechen in der Aktivität und in den
Einstellungen wirken gleich (3.5).

**3.9** Ein Sprachmodell-Download ist wiederaufnehmbar (Spec 0116, Abschnitt 6). Beendet der Nutzer die App mit „Trotzdem beenden“, bleiben die temporären Dateien liegen, und das Modell erscheint beim nächsten Start als „Unterbrochen“. „Fortsetzen“ lädt ab der Größe der vorhandenen temporären Datei weiter (HTTP-Bereichsanfrage); die Prüfsumme wird über die ganze Datei geprüft. Antwortet der Server auf die Bereichsanfrage mit der vollständigen Datei (Status 200), beginnt der Download von vorn, ohne dass eine beschädigte Datei entsteht. „Verwerfen“ löscht die temporären Dateien und den Eintrag. Ein neu gestarteter Download (ohne „Fortsetzen“) beginnt immer bei null.

**3.4** Whisper-Modelldateien einer früheren Version (`ggml-base.bin`,
`ggml-small.bin` und ihre temporären `.part`-Reste im Unterordner
`speech-models`) werden beim ersten Start nach dem Update lokal gelöscht; es
wird nichts aus diesem Unterordner sonst angefasst und keine Netzwerkanfrage
ausgelöst. `base` und `small` zeigen danach „nicht geladen“; die nächste
Nutzung öffnet den Zustimmungsdialog mit der neuen Größe und Quelle. Es gibt
keinen automatischen Download.

## 4. Protokollierung

**4.1** Logs enthalten höchstens Modellname, Größe und Ergebnis, nie
Dateiinhalte, Audio oder erkannten Text.

## 5. Aufnahme und Oberfläche

**5.1** Neben dem Senden-Knopf des Chats steht ein Mikrofon-Knopf. Ein Klick
startet die Aufnahme, ein weiterer Klick beendet sie und startet die
Erkennung; Esc bricht ab und verwirft die Aufnahme. Zusätzlich gibt es einen
Tastaturkurzbefehl zum Starten und Beenden (Abschnitt 5.8).

**5.2** Der erkannte Text wird an den aktuellen Entwurf im Eingabefeld
angehängt. Er wird nie automatisch gesendet; der Nutzer sendet mit Enter.
Schon während der Aufnahme erscheinen die erkannten Wörter nach und nach als
vorläufiger Text hinter dem Entwurf (Leerzeichen getrennt, wenn der Entwurf
nicht leer ist); Wörter können sich dabei noch ändern. Mit *Stop* ersetzt das
Ergebnis einer abschließenden Erkennung über die ganze Aufnahme den
vorläufigen Text; der Entwurf davor bleibt unverändert. Ist das Ergebnis leer,
verschwindet der vorläufige Text. Schlägt die abschließende Erkennung fehl,
bleibt der letzte vorläufige Text stehen und die Fehlermeldung erscheint.
Abbrechen entfernt allen vorläufigen Text und stellt den Entwurf wie vor dem
Start wieder her. Live-Text gibt es immer; es gibt keine Einstellung dafür.

**5.3** Die Sprache wird standardmäßig automatisch erkannt. Eine Einstellung
erlaubt eine feste Sprache. Parakeet erkennt die Sprache immer selbst: Ist
`parakeet_v3` gewählt, wird eine feste Sprache für die Erkennung ignoriert
(weder verändert noch abgelehnt); sie gilt wieder, sobald ein Whisper-Modell
gewählt ist.

**5.4** Der Knopf ist standardmäßig sichtbar. Spracheingabe lässt sich in den
Einstellungen ausschalten; dann ist der Knopf ausgeblendet.

**5.5** Ist noch kein Modell geladen, öffnet der erste Klick einen
Zustimmungsdialog mit Modellgröße und Quelle. Erst nach Zustimmung beginnt
der Download. Schließt der Nutzer den Dialog (Kreuz, Escape) während des
Downloads, läuft dieser weiter (Aktivitätszentrum, 3.8); die Schaltfläche
„Abbrechen“ bricht ihn ab. Ein erneuter Klick während des Downloads zeigt den
Dialog wieder mit dem Fortschritt, statt erneut zu laden.

**5.6** Audio liegt nur im Arbeitsspeicher, wird nie auf die Platte
geschrieben und nie protokolliert. Auch der erkannte Text steht in keinem Log;
Logs nennen höchstens Dauer und Ergebnis.

**5.7** Aufnahme und Erkennung (Befehle für die Oberfläche):
- *Start* öffnet das Mikrofon, und nur dieser Befehl öffnet es. Ist das
  gewählte Modell nicht bereit, endet er mit `speech_no_model`, ohne das
  Gerät zu öffnen. Läuft schon eine Aufnahme, endet er mit
  `speech_already_recording`; die laufende Aufnahme bleibt unberührt.
- *Stop* beendet die Aufnahme, erkennt lokal und liefert den Text. Ohne
  laufende Aufnahme: `speech_not_recording`.
- Während der Erkennung nach einem *Stop* gilt die Aufnahme als laufend:
  *Start* endet mit `speech_already_recording`, ohne das Mikrofon zu öffnen.
- *Abbrechen* verwirft die Aufnahme; es wird nichts erkannt und nichts
  geliefert. Läuft die Erkennung nach einem *Stop* schon, wird ihr Ergebnis
  verworfen und der wartende *Stop* endet mit `speech_not_recording`.
- Die Aufnahme ist auf 120 Sekunden begrenzt. Danach endet sie von selbst und
  die Erkennung läuft; die Oberfläche erhält den Zustand `finished`, und der
  nächste *Stop* liefert den Text.
- Während der Aufnahme erkennt die App im Takt von etwa einer Sekunde den
  bisherigen Ton und meldet den vorläufigen Text als Ereignis
  `speech-partial-text` mit `text` (der gesamte vorläufige Text bisher) und
  `revision` (wächst nur). Es läuft höchstens eine solche Erkennung zugleich;
  eine langsame verzögert das nächste Ereignis, sie wird nicht aufgestaut.
  Je Durchlauf wird nur das jüngste Stück (etwa 25 Sekunden) erkannt, früherer
  Ton wird einmal erkannt und bleibt als fester Text. Nach *Stop* oder
  *Abbrechen* kommen keine solchen Ereignisse mehr; verspätete ignoriert die
  Oberfläche. Das Modell bleibt für eine Aufnahme geladen und wird nach *Stop*
  oder *Abbrechen* freigegeben. Der endgültige Text kommt weiterhin nur als
  Ergebnis von *Stop*, genau einmal.
- Zustandsänderungen kommen als Ereignis: `recording`, `transcribing`,
  `idle`, `finished`. Die Erkennung läuft nicht im Oberflächen-Thread.
- Die Aufnahme ist mono, 16 kHz. Eine Aufnahme unter etwa 0,1 Sekunde ergibt
  leeren Text.
- Sprache: `auto` (Vorgabe) erkennt die Sprache selbst, ein ISO-Code (z. B.
  `de`) legt sie fest. Ein unbekannter Code wird beim Setzen abgelehnt
  (`speech_language_invalid`).
- Fehlerschlüssel: `speech_no_model`, `speech_no_device`,
  `speech_permission_denied`, `speech_transcription_failed`.
- macOS: Der Systemdialog für den Mikrofonzugriff erscheint erst beim ersten
  Start; der Hinweistext erklärt, dass die Erkennung lokal läuft.
- Android: Die Berechtigung „Mikrofon“ wird beim ersten Start abgefragt, bevor
  das Mikrofon geöffnet wird. Verweigert → `speech_permission_denied` (Hinweis
  auf die Systemeinstellungen), keine Aufnahme (Spec 0114).

**5.8** Oberfläche im Einzelnen:
- Der Knopf erscheint nur, wo das Eingabefeld sichtbar ist (nicht ohne
  aktiven KI-Anbieter, nicht in schreibgeschützten Sitzungen) und solange
  Spracheingabe eingeschaltet ist.
- Kurzbefehl: `Strg+Umschalt+M` (`Cmd+Umschalt+M` auf macOS) startet und
  beendet die Aufnahme, solange der Fokus im Chat-Bereich liegt. Er kollidiert
  mit keinem bestehenden Kurzbefehl.
- Während der Aufnahme ist der Knopf rot und pulsiert und zeigt die
  verstrichene Zeit (`m:ss`); während der Erkennung zeigt er einen
  Ladeindikator und ist gesperrt. Alle drei Zustände haben eine
  Screenreader-Beschriftung.
- Vom Start bis der endgültige Text eingefügt ist, ist das Eingabefeld
  schreibgeschützt, der vorläufige Text ist gedämpft und kursiv dargestellt,
  und Senden ist gesperrt. Danach ist das Feld wieder editierbar, hat den
  Fokus, und der Cursor steht am Ende.
- Esc während der Aufnahme verwirft sie; der Entwurf ist danach wieder genau
  wie vor dem Start.
  Verlässt der Nutzer den Chat während der Aufnahme, wird sie verworfen.
- Endet die Aufnahme nach 120 Sekunden von selbst, wird der Text wie bei
  einem Klick auf „Beenden“ übernommen (der vorläufige Text wird ersetzt).
- Der Text kommt wie getippter Text in den Entwurf (angehängt, mit einem
  Leerzeichen getrennt, wenn der Entwurf nicht leer ist) und gilt als
  ungeprüfte Nutzereingabe; er geht erst mit Enter auf dem normalen Sendeweg
  an die KI. Der Fokus wechselt ins Eingabefeld.
- Zustimmungsdialog (ohne bereites Modell): nennt Modell, Größe und Quelle
  (Host und Repository; bei Parakeet zusätzlich der Lizenzhinweis
  „CC-BY-4.0, NVIDIA“) und sagt, dass die Erkennung danach vollständig lokal läuft. „Herunterladen“
  startet den Download mit Fortschrittsanzeige; „Abbrechen“ schließt den
  Dialog ohne Netzwerkanfrage, während des Downloads bricht es ihn ab. Erst
  nach erfolgreichem Download beginnt die Aufnahme (das Betriebssystem fragt
  dann ggf. nach dem Mikrofonzugriff).
- Fehler (kein Mikrofon, Zugriff verweigert, Download fehlgeschlagen,
  Prüfsumme falsch, Erkennung fehlgeschlagen) erscheinen als übersetzter,
  nicht technischer Text mit Hinweis, was zu tun ist; der Knopf kehrt in den
  Ruhezustand zurück.
- Einstellungen, Abschnitt „Spracheingabe“: Ein/Aus (Standard an), Modell
  mit Zustand, Herunterladen/Abbrechen/Löschen (Liste, Größen und Quelle
  stammen aus dem Befehl nach Abschnitt 2.1; Parakeet erscheint als „Parakeet
  v3 (NVIDIA)“ mit Größe und dem Hinweis „mehrsprachig, schnell; erkennt die
  Sprache automatisch“), und die Erkennungssprache
  (automatisch oder eine feste Sprache; ist Parakeet gewählt, steht darunter
  der Hinweis, dass Parakeet die Sprache selbst erkennt und die Einstellung für
  Whisper-Modelle gilt – die Auswahl bleibt bedienbar und der Wert unverändert). Alle Werte bleiben
  über Neustarts erhalten.
- Erkannte Nicht-Sprach-Marker (z. B. „[BLANK_AUDIO]“, „(Musik)“,
  „[silence]“, „*music*“, „♪“) erscheinen weder im Live-Text noch im
  Endergebnis: ganze Segmente aus solchen Markern entfallen, Marker mitten im
  Satz werden entfernt. Nur bekannte Geräusch-Wörter zählen als Marker; vom
  Nutzer Gesagtes mit Klammern („array [0]“) bleibt erhalten. Bei reiner
  Stille bleibt der Entwurf unverändert.

## 6. Sicherheitszusagen

- Kein Download ohne ausdrückliche Handlung des Nutzers.
- Ein Modell wird nur benutzt, wenn seine SHA-256-Prüfsumme gegen den fest
  hinterlegten Wert verifiziert wurde.
- Audio und erkannter Text verlassen den Rechner nicht.

## 6a. Lizenzhinweis

Whisper `base` und `small` von OpenAI, Apache-2.0
(https://huggingface.co/openai/whisper-base, `.../whisper-small`); verwendet
werden die ONNX-Umwandlungen `onnx-community/whisper-base` und `-small` (int8).

Parakeet TDT 0.6B v3 by NVIDIA, CC-BY-4.0
(https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3); verwendet wird die
ONNX-Umwandlung `istupakov/parakeet-tdt-0.6b-v3-onnx` (int8). Der Hinweis
steht in den mitgelieferten Drittlizenz-Hinweisen der App.

## 7. Grenzen

- Nur die Modelle `base`, `small` und (nur Desktop) `parakeet_v3`; eigene
  Modelldateien sind nicht vorgesehen.
- Parakeet läuft nur auf der CPU; eine Aufnahme wird für die Erkennung in
  Stücke von höchstens rund 30 Sekunden geteilt.
- Kein Wiederaufnehmen eines abgebrochenen Downloads: Ein neuer Download
  beginnt von vorn.
