# Spec 0108 — Spracheingabe im KI-Chat

Status: teilweise umgesetzt (Abschnitte 1–4, 5.3,
5.6, 5.7 und 6 sind umgesetzt; der Mikrofon-Knopf, die Tastenkürzel und der
Zustimmungsdialog folgen in einem eigenen Issue; #416 lieferte die
Modellverwaltung, #417 die Aufnahme und Erkennung)
Zweck: Der Nutzer diktiert Text in das Eingabefeld des KI-Chats. Die
Erkennung läuft vollständig auf dem eigenen Rechner mit einem lokalen
Whisper-Modell; Audio verlässt den Rechner nie.
Bezüge: docs/netzwerkverbindungen.md (jede Verbindung ist dort gelistet),
ADR 0086 (keine Inhalte in Standard-Logs).

## 1. Grundsatz

**1.1** Spracheingabe gehört zur Community-Edition.

**1.2** Die Erkennung läuft lokal mit Whisper-Modellen im GGML-Format
(whisper.cpp). Es gibt keinen Cloud-Dienst für die Erkennung; Audio und
erkannter Text werden nie an einen Server geschickt.

**1.3** Die einzige Netzwerkverbindung dieser Funktion ist der Download des
Modells (Abschnitt 3). Sie ist in docs/netzwerkverbindungen.md aufgeführt.

## 2. Modelle

**2.1** Es gibt zwei mehrsprachige Modelle: `base` (rund 142 MiB, Vorgabe) und
`small` (rund 466 MiB).

**2.2** Die Wahl des Modells ist eine Einstellung. Sie bleibt über Neustarts
erhalten, die Vorgabe ist `base`.

**2.3** Jedes Modell hat einen eigenen Zustand: nicht geladen, wird geladen
(mit Fortschritt in Bytes), bereit, Fehler. Wählt der Nutzer ein Modell, das
nicht geladen ist, ist dessen Zustand „nicht geladen“. Die Datei des anderen
Modells bleibt, bis der Nutzer sie löscht.

**2.4** Das Abfragen des Zustands und das Öffnen der Einstellungen lösen
keine Netzwerkanfrage aus.

## 3. Download

**3.1** Das Modell wird nur nach ausdrücklicher Handlung des Nutzers geladen
(Zustimmungsdialog, Abschnitt 5.5). Es gibt keinen Download beim Start und
keine Hintergrund-Aktualisierung.

**3.2** Quelle ist je Modell eine feste URL auf einen unveränderlichen Stand
des Repositories `ggerganov/whisper.cpp` auf huggingface.co (kein
wandernder Branch). Je Modell ist eine SHA-256-Prüfsumme fest hinterlegt.

**3.3** Der Download wird in eine temporäre Datei im Datenverzeichnis der App
geschrieben (Unterordner `speech-models`). Erst nach erfolgreicher Prüfung
von Größe und SHA-256 wird sie atomar unter dem endgültigen Namen abgelegt.
Eine Datei, deren Prüfsumme nicht stimmt, wird gelöscht, als Fehler gemeldet
und nie geladen.

**3.4** Während des Downloads erhält die Oberfläche laufend Fortschritt
(geladene Bytes von insgesamt). Ein zweiter Download desselben Modells
parallel wird abgelehnt.

**3.5** Der Nutzer kann den Download abbrechen. Dann stoppt die Übertragung,
die temporäre Datei wird entfernt und der Zustand ist „nicht geladen“. Auch
ein fehlgeschlagener Download hinterlässt kein Teilmodell.

**3.6** Fehler tragen einen übersetzbaren Schlüssel: Netzwerk, HTTP-Status,
Prüfsumme/Größe, Dateisystem. Der Fehlerzustand bleibt, bis der Nutzer
erneut lädt oder das Modell löscht.

**3.7** Der Nutzer kann ein Modell löschen. Die Datei verschwindet, der
Zustand ist „nicht geladen“. Während eines laufenden Downloads ist Löschen
nicht möglich (erst abbrechen).

## 4. Protokollierung

**4.1** Logs enthalten höchstens Modellname, Größe und Ergebnis, nie
Dateiinhalte, Audio oder erkannten Text.

## 5. Aufnahme und Oberfläche (Soll-Verhalten der Folge-Issues)

**5.1** Neben dem Senden-Knopf des Chats steht ein Mikrofon-Knopf. Ein Klick
startet die Aufnahme, ein weiterer Klick beendet sie und startet die
Erkennung; Esc bricht ab und verwirft die Aufnahme. Zusätzlich gibt es einen
Tastaturkurzbefehl zum Starten und Beenden.

**5.2** Der erkannte Text wird an den aktuellen Entwurf im Eingabefeld
angehängt. Er wird nie automatisch gesendet; der Nutzer sendet mit Enter.

**5.3** Die Sprache wird standardmäßig automatisch erkannt. Eine Einstellung
erlaubt eine feste Sprache.

**5.4** Der Knopf ist standardmäßig sichtbar. Spracheingabe lässt sich in den
Einstellungen ausschalten; dann ist der Knopf ausgeblendet.

**5.5** Ist noch kein Modell geladen, öffnet der erste Klick einen
Zustimmungsdialog mit Modellgröße und Quelle. Erst nach Zustimmung beginnt
der Download.

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

## 6. Sicherheitszusagen

- Kein Download ohne ausdrückliche Handlung des Nutzers.
- Ein Modell wird nur benutzt, wenn seine SHA-256-Prüfsumme gegen den fest
  hinterlegten Wert verifiziert wurde.
- Audio und erkannter Text verlassen den Rechner nicht.

## 7. Grenzen

- Nur die Modelle `base` und `small`; eigene Modelldateien sind nicht
  vorgesehen.
- Kein Wiederaufnehmen eines abgebrochenen Downloads: Ein neuer Download
  beginnt von vorn.
