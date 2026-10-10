# Spec 0034 — Persistente, fortsetzbare KI-Chat-Sitzungen

Status: umgesetzt
Zweck: Chat-Verläufe werden dauerhaft gespeichert und lassen sich beim
erneuten Verbinden zu einem Server fortsetzen.
Bezüge: Spec 0007 und 0021 (Kernschleife), Spec 0006 (Redaction), Spec 0010
(Notiz-Vorschlag beim Beenden), Spec 0016 und 0036 (redigierte,
verschlüsselte Inhalte), Spec 0028/0104 (MCP-Sitzungen sind ausgenommen),
Spec 0039 (Misstrauens-Markierung), Spec 0057 (Kontextaufbau).

## 1. Ziel

Chat-Verläufe liegen lokal vollständig in der Datenbank, nicht nur im
Arbeitsspeicher der laufenden Sitzung. Es gibt keinen anbieterseitigen
Session-Mechanismus: die APIs sind zustandslos, jede Anfrage trägt den
Verlauf selbst.

## 2. Gespeicherte Daten

Je Chat-Sitzung: Server, Titel (leer bis automatisch erzeugt), Start- und
Endzeit (leer, solange aktiv) und der Anbieter, mit dem sie begann.
Je Nachricht: Rolle (Nutzer, Assistent, Aktionsergebnis), Inhaltsart (Text,
Kommando-Ergebnis, abgelehnte Aktion), Inhalt, Reihenfolge, Zeitpunkt.
Löscht man den Server, verschwinden seine Sitzungen und Nachrichten mit.

Der Anbieter einer Sitzung ist rein informativ: Beim Fortsetzen ist
**nicht** derselbe Anbieter nötig, der Textverlauf ist anbieterunabhängig
(Abschnitt 5).

## 3. Was gespeichert wird — redigiert, nicht roh

Gespeichert wird genau das, was durch die Redaction (Spec 0006, Abschnitt
5) gelaufen ist, nicht der Rohinhalt. So sammeln sich in der lokalen
Datenbank nicht zusätzlich unredigierte Secrets an. Vor jedem Senden gilt
zusätzlich Spec 0040, Abschnitt 5.

## 4. Sitzungs-Lebenszyklus

Eine Sitzung beginnt beim Verbinden und endet beim Trennen — eine Sitzung
pro Arbeitsblock, keine endlose Historie pro Server.

- Jede Nachricht (Nutzertext, KI-Antwort, Aktionsergebnis, Ablehnung gemäß
  Spec 0021) wird **sofort** gespeichert, nicht erst am Verbindungsende. Ein
  Absturz verliert höchstens die letzte, noch nicht abgeschlossene
  Nachricht.
- Beim Fortsetzen wird **dieselbe** Sitzung weiterverwendet (Endzeit wird
  geleert, die Reihenfolge läuft weiter) — kein Kopieren. Der Verlauf bleibt
  ein durchgehender Thread über mehrere Verbinden/Trennen-Zyklen.

## 5. „Fortsetzbar" — ohne Ablaufdatum

Eine Sitzung verfällt nicht automatisch nach Zeit. Sie ist fortsetzbar,
wenn sich ihre gespeicherte Historie laden lässt (keine korrupten Daten)
und mindestens ein aktiver KI-Anbieter konfiguriert ist — nicht zwingend
derselbe wie zuvor.

Optional gibt es eine globale **Aufbewahrungs-Einstellung** in Tagen
(Standard: niemals automatisch löschen). Ist sie gesetzt, räumt die App
beim Start Sitzungen auf, deren Endzeit älter als der Zeitraum ist, samt
Nachrichten.

## 6. Ablauf beim Verbinden

Hat ein Server gespeicherte Sitzungen, erscheint beim Klick ein
Auswahl-Screen statt direkten Verbindens:

- **„Neue Unterhaltung"** — prominent, Standardaktion (Enter), beginnt
  eine neue Sitzung.
- **Liste vergangener Sitzungen**, neueste zuerst: Titel (Abschnitt 7),
  Zeitpunkt, Nachrichtenanzahl. Ein Klick lädt die Historie und setzt die
  Sitzung fort (Abschnitt 8).
- Ohne gespeicherte Sitzung: kein Auswahl-Screen, direkt verbinden.

## 7. Automatische Kurztitel

Beim Trennen — sofern die Sitzung mindestens eine Nutzer-Nachricht hat und
noch keinen Titel — wird ein kurzer Titel (2–4 Wörter) erzeugt: ein
minimaler KI-Aufruf ohne Tool-Schema, dessen Antworttext (defensiv auf
sinnvolle Länge begrenzt) der Titel wird. Der Aufruf nutzt denselben
Auslöser wie der Notiz-Vorschlag beim Beenden (Spec 0010, Abschnitt 2).

- Ein gesetzter Titel wird nicht bei jedem weiteren Trennen überschrieben,
  er bleibt stabil.
- Manuelles Umbenennen überschreibt den automatischen Titel dauerhaft.

## 8. Operationen

Auflisten der Sitzungen eines Servers, Fortsetzen, Umbenennen, Löschen.

Fortsetzen baut wie normales Verbinden die SSH-Verbindung auf (inklusive
Host-Key-Bestätigung) und lädt die Historie in den Kontext, bei Bedarf mit
Kürzung (Abschnitt 9). Schlägt das Laden fehl, wird die bereits
aufgebaute Verbindung sauber getrennt. Eine gerade aktive Sitzung lässt
sich nicht löschen (klare Meldung).

## 9. Kontextbegrenzung beim Laden

Eine über Tage fortgesetzte Sitzung kann das Kontextfenster sprengen. Vor
jeder Anfrage wird der gesendete Kontext begrenzt; das Budget ist fest und
kein Bedienknopf. Wie der Kontext aufgebaut und verkleinert wird
(Zusammenfassung alter Runden, Kürzen einzelner Ausgaben), beschreibt
Spec 0057. Der volle Verlauf bleibt unabhängig davon gespeichert.

## 10. Abgrenzung zu MCP

MCP-ausgelöste Aktionen erzeugen **keine** Chat-Sitzungen und schreiben
nichts in einen gespeicherten Verlauf (siehe Spec 0040, Abschnitt 4 und
Spec 0104).

## 11. Neuer Chat innerhalb einer Verbindung

Im Chat-Panel eines verbundenen Tabs gibt es eine Schaltfläche „Neuer
Chat". Sie beginnt einen frischen Chat, ohne die SSH-Verbindung zu trennen:
Terminal, Verbindung, Tab, Dateibrowser, gespeichertes Sudo-Passwort und ein
bereits erhöhter Kanal bleiben unverändert.

1. **Wirkung.** Der bisherige Chat wird beendet (Endzeit gesetzt) und
   bekommt seinen automatischen Titel aus dem bisherigen Verlauf — genau wie
   beim Trennen (Abschnitt 4 und 7). Für denselben Server und den aktiven
   KI-Anbieter entsteht eine neue Sitzung, auf die der Tab umschaltet.
   Verlauf, rollierende Zusammenfassung und Chat-Zustand sind leer, die
   Chat-Ansicht zeigt einen leeren Chat. Der alte Chat erscheint mit
   seinem Titel in der Auswahlliste (Abschnitt 6) und ist wie jeder
   beendete Chat fortsetzbar. Die Ledger-Einträge des alten Chats bleiben
   an diesem; neue Einträge gehören zum neuen Chat.
2. **Misstrauens-Markierung bleibt.** Die Markierung „nicht
   vertrauenswürdiger Inhalt gesehen" (Spec 0039, Abschnitt 5) gilt für die
   Verbindung, nicht für den Chat. Der neue Chat erbt sie und setzt sie nie
   zurück; ebenso bleiben ein offener Injection-Verdacht und eine nicht
   verfügbare Injection-Prüfung bestehen. Ein neuer Chat senkt keine
   Eskalationsstufe.
3. **Gesperrt bei Aktivität.** Die Schaltfläche ist gesperrt, solange in
   diesem Tab eine KI-Antwort läuft oder eine Bestätigung offen ist.
   Dieselbe Prüfung gilt zusätzlich im Backend; dort lehnt der Befehl mit
   einem Fehler ab und ändert nichts.
4. **Titel wie beim Trennen.** Der beendete Chat bekommt Endzeit und
   automatischen Titel (nur wenn er eine Nutzer-Nachricht hat und noch
   keinen Titel) auf demselben Weg wie beim Trennen. Der Vorschlag, Notizen
   zu aktualisieren, gehört zum Ende der Verbindung und wird durch „Neuer
   Chat" nicht ausgelöst.
5. **Ausnahmen.** Ein lokaler Tab hat keinen gespeicherten Chat; dort wird
   nur der Verlauf geleert, es entsteht keine Sitzung. MCP-Sitzungen
   (Abschnitt 10) zeigen die Schaltfläche nicht. Die einleitende
   Betriebssystem-Information der Verbindung bleibt im Kontext erhalten.
   Schlägt das Anlegen der neuen Sitzung fehl, bleibt der bisherige Chat
   unverändert aktiv und die Schaltfläche meldet den Fehler.

## 12. Grenzen

- Der Chat des lokalen Pseudo-Servers wird nicht gespeichert (er hat keinen
  Server-Eintrag, an den die Sitzung gebunden wäre).
- Ältere Nachrichten werden beim Senden gekürzt bzw. zusammengefasst
  (Spec 0057), nicht aus dem gespeicherten Verlauf gelöscht.
