# Spec 0063 — Diagnose-Export in der App

Status: umgesetzt
Zweck: Ein Nutzer erzeugt mit einem Klick ein redigiertes Diagnosepaket (Version und Build-Kennung, System, Datenpfade, App-Zustand, unkritische Log-Zeilen), das er an den Support oder in ein Issue hängen kann, ohne dabei Geheimnisse oder Nutzerinhalte preiszugeben.
Bezüge: Spec 0016 (strukturiertes Logging), Spec 0094 (kein Inhalt in Standard-Logs), Spec 0006 und Spec 0095 (Redaction), Spec 0052 (Version und Build-Kennung), Spec 0050 (Einstellungen, Kategorie „Diagnose"), Spec 0071 (Anzeige des Schlüsselbund-Zustands), Spec 0101 (Secrets nur in der Datenbank).

## 1. Zwei Funktionen

Beide stehen in den Einstellungen in der Kategorie „Diagnose":

- **„Diagnose-Logs im Dateimanager öffnen"** öffnet den Ordner mit den
  Logdateien im Dateimanager des Betriebssystems. Existiert der Ordner nicht,
  erscheint ein Fehler statt eines leeren Fensters.
- **„Diagnosepaket erzeugen"** stellt das Paket zusammen (§2) und zeigt es
  zur Durchsicht an (§3). Erst danach bietet „Speichern unter …" einen
  Speichern-Dialog an.

Dieselbe Kategorie zeigt außerdem, ob der Systemschlüsselbund verfügbar ist
(Spec 0071), und die Datenpfade der App.

## 2. Was ins Paket kommt

Das Paket ist ein einzelner Markdown-Text, gegliedert in die Abschnitte
Version, System, Datenpfade, App-Zustand und Letzte Log-Zeilen. Er beginnt mit
dem Hinweis, dass er lokal erzeugt und **nicht** versendet wurde und vor dem
Weitergeben durchzusehen ist.

- **Version:** Versionsnummer mit Build-Kennung (Spec 0052), Edition
  (Community oder Official) und Build-Art (Dev oder Release).
- **System:** Betriebssystem, Architektur und, soweit ermittelbar, die
  Betriebssystemversion; sonst „unbekannt". Eine fehlende Betriebssystem-
  version bricht den Export nicht ab.
- **Datenpfade:** Datenbank, Log-Ordner und Host-Key-Speicher.
- **App-Zustand:** nur die **Typen** der konfigurierten KI-Provider (z. B.
  „anthropic") und die **Anzahl** der Server. Konnte ein Wert nicht gelesen
  werden, steht „unbekannt (Fehler beim Lesen)" — das ist etwas anderes als
  „keine" bzw. 0 und wird nicht verwechselt.
- **Letzte Log-Zeilen:** bis zu 500 Zeilen der zuletzt beschriebenen
  Logdatei (nur Dateien, die als Logdateien der App erkennbar sind), gefiltert
  und redigiert nach §3. Gibt es keine, steht „(keine Log-Zeilen verfügbar)".
- **Nie enthalten:** API-Keys, Passwörter, Passphrasen, Host-Keys,
  Server-Adressen und Zugangsdaten, Notiz-Inhalte, Chat-Inhalte,
  Kommandotexte und -ausgaben. Das Paket beschreibt den Zustand der App, nicht
  die Daten des Nutzers. Auch Provider-Adressen, Schlüsselverweise und
  Zusatz-Header gehören nicht dazu.

## 3. Schutz der Log-Zeilen und Vorschau

Zwei voneinander unabhängige Schichten, in dieser Reihenfolge:

1. **Positivliste.** Nur Log-Zeilen mit einer ausdrücklich zugelassenen
   Meldung (Start- und Ablaufereignisse, Fehlermeldungen ohne Nutzerinhalt,
   Metadaten zu KI-Anfragen wie Status und Längen) kommen ins Paket. Jede
   andere Zeile entfällt vollständig — auch wenn sie harmlos wäre. Eine Zeile,
   die sich nicht als strukturierte Log-Zeile mit erkannter Meldung lesen
   lässt, entfällt ebenfalls. Neue Log-Zeilen mit Nutzerinhalt landen damit
   nie von selbst im Paket.
2. **Redaction.** Was die Positivliste durchlässt, läuft zusätzlich durch
   dieselbe Redaction wie überall in der App (Spec 0006, Spec 0095) — auch
   wenn die Logs schon beim Schreiben redigiert wurden. Das ist ein
   Sicherheitsnetz, weil das Paket oft öffentlich geteilt wird.

**Vorschau vor dem Teilen.** Das Paket erscheint zuerst in einem schreibgeschützten
Textfeld mit dem Hinweis, es vor dem Teilen durchzusehen (Log-Zeilen sind
begrenzt und redigiert, Zugangsdaten, Chat- und Notiz-Inhalte sind nicht
enthalten, Pfade enthalten den Benutzernamen). Gespeichert wird erst nach einer
bewussten Aktion, und genau der angezeigte Text — er wird nicht erneut
zusammengestellt. Bricht der Nutzer den Speichern-Dialog ab, geschieht nichts.
Die Vorschau ist die zweite Verteidigungslinie für Geheimnisse, die ein
musterbasierter Redactor nicht kennt.

## 4. Format

Eine einzelne Textdatei im Markdown-Format (Vorschlag für den Dateinamen:
„smart-ssh-diagnose.md"), leicht in ein Issue einzufügen.

## 5. Sicherheitszusagen

- Keine Geheimnisse und keine Nutzerinhalte im Paket (§2): Das Paket wird aus
  einer festen, kleinen Feldliste zusammengesetzt; alles andere kann es nicht
  enthalten.
- Log-Zeilen laufen durch Positivliste **und** Redaction (§3); im Zweifel
  fehlt eine Zeile, statt eine sensible durchzulassen.
- Kein automatisches Speichern oder Versenden; der Nutzer sieht den Text vor
  dem Speichern.
- Das Erzeugen ändert keine Daten (rein lesend).
- Die Ursachenkette eines Schlüsselbund-Fehlers (nur fürs Log) gehört nicht
  ins Paket.

## 6. Grenzen

- Die Redaction ist musterbasiert; ein unbekanntes Geheimnisformat erkennt sie
  nicht. Dafür gibt es die Positivliste und die Vorschau.
- Die Betriebssystemversion kommt aus einem Systemkommando und ist deshalb nur
  eine Näherung.
- Es wird nur die zuletzt beschriebene Logdatei ausgewertet, nicht ältere.
