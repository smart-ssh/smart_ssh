# Spec 0021 — Fortsetzung nach Aktionsergebnis

Status: umgesetzt
Zweck: Nach jedem Ausgang einer vorgeschlagenen Aktion erfährt die KI das
Ergebnis und kann ohne manuelles Anstoßen weiterdenken — ohne dass dadurch
Kontrolle über neue Kommandos verloren geht.
Bezüge: Spec 0007 (Kernschleife), Spec 0002 (Filter-Engine), Spec 0006.

## 1. Grundsatz

Die Automatisierung betrifft ausschließlich, dass die KI Ergebnisse sieht
und weiterdenken darf. Sie führt **nicht** dazu, dass künftige Kommandos
automatisch ausgeführt werden: Jeder neue Kommandovorschlag durchläuft
unverändert die Filter-Engine mit allen Konsequenzen (automatisch
ausführen, bestätigen lassen, blockieren).

## 2. Ziel

Ein einheitliches Fortsetzungsverhalten für alle vier Ausgänge einer
vorgeschlagenen Aktion (Abschnitt 3), damit der Chat nach einer Ablehnung
nicht in einem Warte-Zustand hängen bleibt und die KI nach jedem Schritt
reagieren kann.

## 3. Die vier Ausgänge

Für jeden Fall gilt: Ein Ergebnis-Eintrag kommt in den Verlauf, **danach
folgt automatisch** eine neue Anfrage an die KI, ohne dass der Nutzer
tippen muss. Die Fälle unterscheiden sich nur im Inhalt des Eintrags:

1. **Automatisch ausgeführt:** Kommando und Kommando-Ergebnis.
2. **Bestätigt und ausgeführt** (auch nach Bearbeiten): wie Fall 1, nach der
   Ausführung.
3. **Vom Nutzer abgelehnt:** Es wird nichts ausgeführt; der Eintrag sagt der
   KI ausdrücklich, dass *der Nutzer* diesen Vorschlag abgelehnt hat (nicht
   die Filter-Engine). Die KI kann eine Alternative vorschlagen, nachfragen
   oder einen anderen Ansatz verfolgen.
4. **Von der Filter-Engine blockiert** (kein Dialog, Spec 0002): Der Eintrag
   nennt den Blockier-Grund, damit die KI nicht denselben Vorschlag später
   wiederholt.

Eine vom Nutzer bestätigte und gespeicherte Notiz-Änderung erscheint für die
KI als eigener, ausgezeichneter Ergebnis-Eintrag: Er sagt, dass der Vorschlag
der KI bestätigt und angewendet wurde, nennt das Ziel (aktueller Server oder
Servergruppe) mit seinem Namen und gibt den gespeicherten (bereits
geschwärzten) Notiztext wieder, in der Sprache der Sitzung. Er wird nie mit
Text des Nutzers zu einer Nachricht verschmolzen (etwa einer Frage, die der
Nutzer bei offener Karte getippt hat). Die automatische Fortsetzung sieht
die neue Notiz im System-Prompt.

Abgelehnte Notiz-Änderungen folgen demselben Muster (Ergebnis-Eintrag und
automatische Fortsetzung); für die KI ist das dieselbe Art Rückmeldung wie
ein abgelehntes Kommando.

## 4. Sicherheits-Cap gegen Endlosschleifen

Die App zählt die automatischen Fortsetzungsrunden **seit der letzten
Nutzer-Beteiligung**; das Limit ist fest 10 und nicht konfigurierbar. Bei
10 Runden in Folge ohne Nutzer-Beteiligung stoppt die Automatik, eine
sichtbare Chat-Systemnachricht erscheint („Automatische Fortsetzung nach 10
Schritten ohne deine Bestätigung angehalten — schreib weiter, um
fortzufahren"). Den Zähler auf 0 setzen: eine neue Nutzer-Nachricht, eine
während der Automatik eingereihte Nutzer-Nachricht, die in eine Runde
eingespeist wird, und jede Entscheidung des Nutzers über einen
Bestätigungsdialog (Genehmigen, Bearbeiten und Genehmigen, Ablehnen). Das
Zeitlimit eines Dialogs, eine Ablehnung wegen Wiederverbindung, ein
verworfener Dialog, ohne Dialog ausgeführte Aktionen (Regel erlaubt),
blockierte Aktionen und ungültige Werkzeugaufrufe setzen den Zähler nicht
zurück; KI-Ausgabe oder Kommando-Ausgabe können ihn nie zurücksetzen. Wer
also mindestens alle 10 Runden eine Bestätigung entscheidet, lässt eine
lange Aufgabe durchlaufen.

## 5. Sichtbare Kontrolle

Läuft eine automatische Fortsetzungsrunde (die KI antwortet auf ein
Aktionsergebnis, ohne dass der Nutzer getippt hat), zeigt die Oberfläche
einen klar sichtbaren **„Automatik läuft"**-Indikator mit einem
**„Automatik stoppen"**-Button. Ein Klick bricht die Fortsetzungskette für
diese Nutzer-Nachricht sofort ab, unabhängig vom Zähler. Das stoppt nur das
automatische Weiterreden: bereits anstehende Bestätigungsdialoge bleiben
bestehen, bis der Nutzer sie entscheidet.

## 6. Darstellung im Verlauf

Abgelehnte und blockierte Vorschläge bleiben im Verlauf sichtbar —
durchgestrichen bzw. mit dem Label „Abgelehnt"/„Blockiert", konsistent mit
Spec 0007 (auch blockierte Fälle werden transparent gezeigt, nie still
verworfen).

## 7. Eingabe bleibt nutzbar

Nach **jedem** der vier Ausgänge ist die Eingabe wieder nutzbar, auch wenn
die automatische Fortsetzung nicht greift (z. B. Netzwerkfehler bei der
Folge-Anfrage). Ein Warte-Zustand darf nach einer Ablehnung nie hängen
bleiben.

## 8. Grenzen

- Das Fortsetzungslimit (Abschnitt 4) ist nicht einstellbar.
