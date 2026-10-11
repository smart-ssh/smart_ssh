# Spec 0107 — Verlorene Sitzung im selben Tab neu verbinden

Status: umgesetzt · Issue: #355
Zweck: Bricht die Verbindung zu einem Server ab, verbindet der Nutzer den
selben Tab mit einem Klick auf „Neu verbinden“ oder der Taste R neu. Chat-
Verlauf und KI-Kontext bleiben, Terminal und Dateibrowser kommen zurück.
Review-Priorität: ERHÖHT (Verbindungsaufbau, Host-Key, Zugangsdaten, erhöhter Kanal)

Bezüge: Spec 0007 (Sitzungen, Terminal), Spec 0017 (Tabs), Spec 0057
(Protokoll), Spec 0067 (erhöhter Dateizugriff), Spec 0102 (Startverzeichnis),
Spec 0104 (MCP-Sitzungen), ADR 0007 (Host-Key-Bestätigung), ADR 0132.

## 1. Auslöser

**1.1** Ein getrennter Sitzungs-Tab zeigt im Kopf neben dem Hinweis
„Verbindung getrennt“ den Knopf „Neu verbinden“. Er ist mit Tab erreichbar.

**1.2** Im Terminal steht die Zeile „Verbindung verloren. R drücken, um neu
zu verbinden.“ Drückt der Nutzer R oder r, während das tote Terminal den
Fokus hat, startet der Neuverbindungsvorgang. Alle anderen Tasten tun nichts
und gehen nicht an den Server. In einem lebenden Terminal geht R wie jede
andere Taste an die Shell.

**1.3** Läuft ein Neuverbinden, zeigt der Knopf „Verbinde neu …“ und ist
gesperrt. Ein zweiter Auslöser (Doppelklick, gehaltene Taste) startet keinen
zweiten Verbindungsversuch. Eine nicht getrennte Sitzung lässt sich nicht
neu verbinden.

## 2. Was bleibt, was geht

**2.1 Bleibt:** Tab, Sitzungs-ID, Chat-Verlauf, der KI-Kontext samt
Zusammenfassung und Systemkontext, das Protokoll.

**2.2 Geht:** der alte Transport und der normale Dateikanal (der neue öffnet
sich beim ersten Zugriff), der erhöhte Dateikanal (der Nutzer erhöht erneut),
die lokalen Pfadfreigaben dieser Sitzung (Issue #89).

**2.3 Wartende Bestätigung.** Eine zum Zeitpunkt des Abbruchs wartende
Bestätigung wird vor dem Neuverbinden abgelehnt. Sie wird nie auf der neuen
Verbindung ausgeführt. Ein laufendes KI-Kommando wird abgebrochen.

## 3. Verbindungsaufbau

Das Neuverbinden geht genau den Weg eines ersten Verbindens zum selben
Serverprofil: Jump-Hosts, Zugangsdaten aus dem Schlüsselbund, Host-Key-
Prüfung. Es gibt keine gemerkte Zustimmung: Ein geänderter oder unbekannter
Host-Key zeigt wieder den Host-Key-Dialog; lehnt der Nutzer ab, bleibt der
Tab getrennt. Fehlende Zugangsdaten führen zu demselben Fehler wie beim
ersten Verbinden, es gibt keine Passwortabfrage.

Das Sudo-Passwort wird neu aus dem Schlüsselbund gelesen und für die
Schwärzung von Ausgaben verwendet.

## 4. Nach dem Erfolg

**4.1** Im selben Terminal öffnet sich eine neue Shell. Der bisherige
Verlauf bleibt stehen, getrennt durch die Zeile „— neu verbunden —“. Ein
konfiguriertes Startverzeichnis wird wieder angewendet (Spec 0102).

**4.2** Der Dateibrowser lädt sein bisheriges Verzeichnis neu, im Home, wenn
es nicht mehr existiert. Er startet im normalen Modus.

**4.3** Die nächste KI-Nachricht geht mit dem bisherigen Verlauf raus.

**4.4** Eine so wieder verbundene Sitzung gilt für MCP wieder als
wiederverwendbare, verbundene Sitzung (Spec 0104).

## 5. Fehler

Scheitert das Neuverbinden, zeigt der kleine Hinweis im Tab-Kopf den
übersetzten Fehler (Spec 0024). Der Tab bleibt getrennt, der Knopf bleibt
nutzbar, die R-Taste auch.

## 6. Status-Ereignis

`connection-status-changed` kennt zusätzlich den Zustand `reconnecting`. Er
geht bei Erfolg auf `connected`, bei Misserfolg zurück auf `disconnected`.
Die Sitzungsliste meldet während des Neuverbindens ebenfalls `reconnecting`.

## 7. Nicht Teil dieser Spec

- Automatisches Neuverbinden ohne Zutun des Nutzers.
- Wiederherstellen des erhöhten Dateikanals, der lokalen Pfadfreigaben oder
  einer Shell-Sitzung (Umgebung, laufende Prozesse).
- Passwortabfrage bei fehlenden Zugangsdaten.
