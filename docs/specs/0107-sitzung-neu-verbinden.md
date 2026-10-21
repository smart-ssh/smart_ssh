# Spec 0107 — Verlorene Sitzung im selben Tab neu verbinden

Status: umgesetzt · Issues: #355, #356
Zweck: Bricht die Verbindung zu einem Server ab, verbindet die App den
selben Tab automatisch neu (Abschnitt 8) oder der Nutzer mit einem Klick auf
„Neu verbinden“ oder der Taste R. Chat-
Verlauf und KI-Kontext bleiben, Terminal und Dateibrowser kommen zurück.
Review-Priorität: ERHÖHT (Verbindungsaufbau, Host-Key, Zugangsdaten, erhöhter Kanal)

Bezüge: Spec 0007 (Sitzungen, Terminal), Spec 0017 (Tabs), Spec 0057
(Protokoll), Spec 0067 (erhöhter Dateizugriff), Spec 0102 (Startverzeichnis),
Spec 0104 (MCP-Sitzungen), ADR 0007 (Host-Key-Bestätigung), ADR 0132, ADR 0133.

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

Das Sudo-Passwort wird neu aus dem Schlüsselbund gelesen und gilt danach
sowohl für `sudo`-Eingaben als auch für die Schwärzung von Ausgaben; der
alte Wert wird nicht weiterverwendet.

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

**6.1 Neue Shell und Verlustüberwachung (Nachtrag).** Nach jedem erfolgreichen
Neuverbinden – manuell oder automatisch – öffnet das Terminal genau eine neue
Shell und schreibt die Trennlinie. Der Verlust-Hinweis erscheint auch dann,
wenn der Verlust zuerst als `reconnecting` gemeldet wird. Die
Verbindungsüberwachung läuft für jede Verbindung, die eine Sitzung hält, auch
nach einem Neuverbinden: ein erneuter Verlust wird wie der erste erkannt
(Status `disconnected`, automatisches Neuverbinden, Knopf). Die Überwachung
der ersetzten Verbindung meldet nichts für die neue.

## 7. Nicht Teil dieser Spec

- Wiederherstellen des erhöhten Dateikanals, der lokalen Pfadfreigaben oder
  einer Shell-Sitzung (Umgebung, laufende Prozesse).
- Passwortabfrage bei fehlenden Zugangsdaten.
- Automatisches Neuverbinden von MCP-Sitzungen (Spec 0104).

## 8. Automatisch neu verbinden

**8.1 Einstellung.** „Automatisch neu verbinden“ in den Einstellungen
(„Sitzungen & Daten“): Aus, 30 Sekunden, 60 Sekunden, 5 Minuten; Standard
60 Sekunden. Sie bleibt über Neustarts erhalten und gilt ab dem nächsten
Verbindungsabbruch. Ein unbekannter gespeicherter Wert gilt als Standard.

**8.2 Auslöser.** Nach jedem unerwarteten Verbindungsabbruch (Terminal-Ende,
Keepalive, andere Verlustmeldung des Transports) versucht die App von selbst
neu zu verbinden, außer die Einstellung steht auf Aus; dann erscheint der
Knopf sofort. Ein vom Nutzer ausgelöstes Trennen und das Schließen des Tabs
starten keine Versuche und beenden laufende.

**8.3 Zeitplan.** Die Wartezeiten vor den Versuchen sind 2, 4, 8 und danach
immer 15 Sekunden, gerechnet bis zum Ablauf der eingestellten Zeit. Es läuft
höchstens ein Versuch je Sitzung. Der Kopf zeigt „Verbinde neu … (Versuch N)“
und einen Link „Stopp“, der die Versuche sofort beendet; danach erscheint der
Knopf „Neu verbinden“. Im Terminal steht die Zeile „R drücken“ erst, wenn die
Versuche enden. Ein Klick auf den Knopf oder R während der Versuche tut
nichts.

**8.4 Ende ohne Erfolg.** Die Versuche enden sofort, und der Knopf erscheint,
wenn ein Versuch den Nutzer braucht oder Warten nicht hilft: unbekannter oder
geänderter Host-Key, fehlende Zugangsdaten oder gesperrter Schlüsselbund,
abgelehnte Anmeldung (jeder andere Fehler außer Netz- und Zeitüberschreitungs-
fehlern ebenso). Ein automatischer Versuch öffnet nie den Host-Key-Dialog und
vertraut nie einem Schlüssel; der Dialog erscheint erst, wenn der Nutzer
„Neu verbinden“ klickt (Abschnitt 3). Läuft die Zeit ab, zeigt der Kopf
„Verbindung konnte nicht wiederhergestellt werden.“ mit dem Knopf, das
Terminal die Zeile „R drücken“.

**8.5 Erfolg.** Gelingt ein Versuch, ist der Tab ohne Zutun wieder verbunden
(Abschnitt 4).

**8.6 KI-Fortsetzung.** War beim Abbruch ein KI-Turn aktiv (Kommando läuft,
Bestätigung wartet oder Folgerunde steht an), bekommt die KI nach dem
automatischen Neuverbinden den Hinweis „Die Verbindung zum Server ging
verloren und wurde wiederhergestellt. Das letzte Kommando wurde möglicherweise
nicht abgeschlossen; prüfe seine Wirkung, bevor du es wiederholst.“ (in der
Sprache des System-Prompts) und eine Folgerunde startet. Das unterbrochene
Kommando wird nicht wiederholt; jeder neue Vorschlag läuft durch Filter und
Bestätigung wie sonst. Drückt der Nutzer während des Abbruchs Stopp
(Spec 0066), entfällt die Fortsetzung. War kein Turn aktiv, geht nichts an die
KI. Die Fortsetzung gibt es höchstens einmal je Abbruch; bricht die Verbindung
in der Folgerunde erneut ab, beginnt ein neuer Zyklus nach denselben Regeln.
Nach einem manuellen Neuverbinden gibt es keine Fortsetzung.

**8.7 Ereignis.** Das Frontend erfährt den Stand über das Ereignis
`auto-reconnect-changed` (Phase `running` mit Versuchsnummer, `reconnected`,
`gave_up`, `stopped`, `blocked`); es kommt vor dem Statuswechsel auf
`disconnected`.
