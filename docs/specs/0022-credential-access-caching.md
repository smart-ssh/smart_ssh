# Spec 0022 — Credentials werden pro Sitzung einmal gelesen

Status: umgesetzt
Zweck: Ein Secret wird nicht bei jeder Aktion neu aus dem Speicher geholt, sondern einmal je Sitzung bzw. je Provider-Instanz; unter macOS entstehen dadurch und durch eine stabile Entwicklungssignatur möglichst wenige Schlüsselbund-Abfragen.
Bezüge: Spec 0003 (Credentials), Spec 0018 (Sudo-Passwort), Spec 0101 (Ablage der Secrets), ADR 0022 (stabile Dev-Signatur).

## 1. Hintergrund

macOS fragt nach, wenn eine App auf ein Schlüsselbund-Element zugreift, dessen
Freigabe an die Code-Signatur gebunden ist. Seit Spec 0101 liegt im
Schlüsselbund höchstens noch der Wurzelschlüssel der Datenbank; alle anderen
Secrets liegen in der verschlüsselten Datenbank. Wiederholte Abfragen entstehen
daher vor allem durch (a) eine bei jedem Neubau wechselnde Signatur im
Entwicklungs-Build und (b) unnötig wiederholte Lesezugriffe.

## 3. Einmal lesen

- **API-Schlüssel des KI-Anbieters:** wird beim Aufbau der Anbieter-Instanz
  einmal gelesen und für deren Lebensdauer gehalten, nicht je Anfrage. Beim
  Wechsel des aktiven Anbieters wird neu gelesen, sonst nicht.
- **Login-Credential und Sudo-Passwort:** werden beim Verbinden einmal
  gelesen und für die Sitzung gehalten, nicht je ausgeführtem Kommando
  (Spec 0018).
- Der Wert wird nur im Arbeitsspeicher des laufenden Prozesses gehalten. Es gibt
  keinen zusätzlichen Zwischenspeicher auf der Platte.

## 4. Stabile Dev-Signatur (macOS)

- Entwicklungs-Builds werden mit einer stabilen, projektspezifischen Identität
  signiert, damit eine einmal erteilte Freigabe über mehrere Läufe hinweg
  gültig bleibt (ADR 0022). Beitragende starten die Entwicklungs-App unter
  macOS mit `./scripts/tauri-dev.sh`.
- Das gilt nur für die lokale Entwicklung und ersetzt keine Developer-ID-
  Signatur für Veröffentlichungen; Pakete aus diesem Repository werden nicht signiert (Spec 0090).

## 5. Grenzen

- Ein Cache über einen Neustart hinaus gibt es nicht.
- Eine Änderung eines Secrets durch den Nutzer wirkt für laufende Sitzungen erst
  mit der nächsten Verbindung.
