# 0021-turn-continuation-design

## Status
Vorgeschlagen

## Kontext

`docs/specs/0021-turn-continuation.md` beschreibt das Fortsetzungsverhalten
nach einem Aktionsergebnis (Abschnitt 3), ein Sicherheits-Cap (Abschnitt 4)
und eine manuelle Stopp-Möglichkeit (Abschnitt 5), lässt aber die konkrete
technische Umsetzung offen. Drei Punkte mussten dabei ohne explizite
Spec-Vorgabe entschieden werden.

## Entscheidungen

**1. Wiederverwendung des bestehenden Runden-Zählers statt eines zweiten,
parallelen Mechanismus — Default dabei von 25 zurück auf 10 gesenkt.**
`run_chat_turn` hatte bereits eine Runden-Schleife samt Zähler
(`MAX_AUTO_FOLLOWUP_ROUNDS`, ADR 0014), die bislang nur weiterlief, wenn
eine Aktion *tatsächlich ausgeführt* wurde. Statt einen zweiten Zähler nur
für "abgelehnte/blockierte Runden" einzuführen, wurde dieselbe Schleife so
erweitert, dass sie nach jedem der vier Ausgänge aus Abschnitt 3 weiterläuft
— ein Vorschlag pro Runde reicht. Der Zähler zählt jetzt also mehr als
vorher (auch Ablehnungen/Blockierungen), weshalb der alte Wert 25
(bewusst hochgesetzt, weil legitime mehrstufige *Ausführungs*-Ketten sonst
zu früh abbrachen) nicht mehr dieselbe Rechtfertigung hat: abgelehnte/
blockierte Runden erledigen sich quasi sofort (kein Warten auf einen
entfernten Prozess), und Spec 0021 nennt ausdrücklich "Default-Limit 10".
Das Erreichen des Caps ist zudem kein harter Fehler mehr, sondern ein
weicher Stopp mit Fortsetzungsmöglichkeit per neuer Nachricht — zusätzlich
zur jetzt vorhandenen manuellen Stopp-Möglichkeit (Punkt 2) ist ein
großzügigerer Automatik-Zähler weniger wichtig als vorher.

**2. "Automatik stoppen" über ein `AtomicBool`-Flag auf `Session`, geprüft
nur *zwischen* Runden — kein Umbau von `send_chat_message` auf
Fire-and-Forget.** `send_chat_message` bleibt bewusst synchron über die
komplette Runden-Kette hinweg (unverändert gegenüber vorher). Tauris IPC
ist ohnehin voll asynchron: ein zweiter, unabhängiger Command
(`stop_auto_continuation`) kann jederzeit aufgerufen werden, während der
erste noch läuft — ein Neuentwurf auf einen im Hintergrund gespawnten Task
wäre nur nötig gewesen, um den Stop-Button *technisch* zu ermöglichen, was
er aber ohnehin schon ist. Der Vorteil dieser einfacheren Lösung: die
Prüfung liegt strukturell außerhalb von `run_one_round`, ein bereits
offener Bestätigungsdialog kann dadurch gar nicht erst unterbrochen werden
(Abschnitt 5, letzter Satz) — ganz ohne zusätzlichen Sonderfall dafür.

**3. Ein neues Ereignis `chat-auto-continuation-started` statt einer
Herleitung aus bestehenden Ereignissen, aber kein zweites "beendet"-
Ereignis.** Das Frontend muss wissen, *wann* eine automatische Runde
beginnt (für den Indikator) — das ließe sich zwar aus der Ereignisfolge
ableiten (z. B. "ein weiteres `chat-action-proposed` nach einem bereits
verarbeiteten Ergebnis, ohne neue Nutzer-Nachricht"), wäre aber fragile,
dupliziert Backend-Zustand im Frontend nach. Für das *Ende* der Automatik
reicht dagegen die bereits vorhandene `sendChatMessage()`-Promise-Auflösung
(Grundlage von `sending`) — sie ist zuverlässig, weil `send_chat_message`
synchron über die komplette Kette läuft (Punkt 2): egal ob die Kette
regulär endet, das Cap erreicht oder manuell gestoppt wird, das Promise
löst in jedem Fall auf. Ein zweites Ereignis nur dafür hätte keinen
Mehrwert gebracht.

## Konsequenzen

**Positiv:**
- Kein zweiter, parallel zu pflegender Zähler-Mechanismus.
- Der Stop-Button lässt sich ohne Architektur-Umbau der Kernschleife
  hinzufügen, mit der geforderten "lässt offene Dialoge unangetastet"-
  Eigenschaft praktisch geschenkt durch die Platzierung der Prüfung.
- Nur ein neues Ereignis nötig statt zwei.

**Negativ / Trade-off:**
- Die Eingabe bleibt für die *gesamte* Dauer einer automatischen
  Fortsetzungskette gesperrt (da `send_chat_message` synchron bleibt) —
  bei einem sehr langsamen KI-Provider könnte das ohne den Stop-Button
  unangenehm lang wirken. Der Stop-Button federt das ab, ersetzt aber kein
  echtes "im Hintergrund weiterlaufen, während man etwas anderes tut".
- Das gesenkte Rundenlimit (10 statt 25) könnte einzelne, sehr lange
  legitime Ausführungsketten (viele tatsächlich ausgeführte Schritte
  hintereinander) wieder häufiger zum Cap führen als mit 25 — abgefedert
  durch die neue manuelle Fortsetzung per einfacher neuer Nachricht sowie
  den expliziten Spec-0021-Wunsch nach genau diesem Default.
