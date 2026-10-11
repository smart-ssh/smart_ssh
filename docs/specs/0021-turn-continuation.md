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

Abgelehnte Notiz-Änderungen folgen demselben Muster (Ergebnis-Eintrag und
automatische Fortsetzung); für die KI ist das dieselbe Art Rückmeldung wie
ein abgelehntes Kommando.

## 4. Sicherheits-Cap gegen Endlosschleifen

Pro Nutzer-Nachricht zählt die App die automatischen Fortsetzungsrunden;
das Limit ist fest 10. Bei Erreichen stoppt die Automatik, eine sichtbare
Chat-Systemnachricht erscheint („Automatische Fortsetzung nach 10 Schritten
angehalten — schreib weiter, um fortzufahren"). Eine neue Nutzer-Nachricht
setzt den Zähler zurück.

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
