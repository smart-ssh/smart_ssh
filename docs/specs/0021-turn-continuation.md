# Spec: Turn-Fortsetzung nach Aktionsergebnis (Agentic-Loop-Abschluss)

Status: Entwurf
Modul: Erweiterung `crates/app-tauri` (Kernschleife), `frontend/`
(Chat-Verlauf-Darstellung, Warte-Zustand)
Abhängigkeiten: Kernschleife (Spec 0007, Abschnitt 6), Filter-Engine (Spec
0002), KI-Provider (Spec 0006)

## 1. Problem

Spec 0007, Abschnitt 6 beschreibt für `AutoExec`, dass das Ausführungs-
ergebnis in `context.history` übernommen wird — aber **nicht explizit**,
ob und wann danach automatisch ein neuer Request an die KI geht, damit sie
auf das Ergebnis reagieren kann. Für `Confirm` mit anschließender
Ablehnung durch den Nutzer ist das Verhalten in Spec 0007 **gar nicht**
definiert. Diese Lücke ist vermutlich die Ursache für das gemeldete
Verhalten: Nach "Ablehnen" bekommt die KI nie mitgeteilt, dass abgelehnt
wurde, kein Folge-Request wird ausgelöst, und der Chat bleibt in einem
Warte-Zustand hängen, der nie aufgelöst wird.

## 2. Ziel

Ein einheitliches, für alle vier möglichen Ausgänge einer vorgeschlagenen
Aktion geltendes Fortsetzungsverhalten — konsistent mit dem
ursprünglichen Produktziel ("die KI leitet einen durch den Prozess, prüft
laufende Prozesse usw.", ohne dass man nach jedem einzelnen Schritt manuell
erneut anstoßen muss), aber **ohne** die Kontrollprinzipien aus Spec 0002/
0007 aufzuweichen: Die Automatisierung betrifft ausschließlich, dass die KI
Ergebnisse automatisch sieht und weiterdenken darf — **nicht**, dass
künftige Kommandos automatisch ausgeführt werden. Jeder neue
`SuggestCommand`-Vorschlag durchläuft weiterhin unverändert die
Filter-Engine, mit allen bisherigen Konsequenzen (`AutoExec`/`Confirm`/
`Deny`).

## 3. Die vier Ausgänge und ihr Fortsetzungsverhalten

Für jeden der folgenden vier Fälle gilt: Ein Ergebnis-Eintrag wird in
`context.history` aufgenommen, **danach automatisch** ein neuer
`AiProvider::send()`-Aufruf ausgelöst — ohne dass der Nutzer etwas tippen
muss. Der einzige Unterschied zwischen den Fällen ist der Inhalt des
Ergebnis-Eintrags:

1. **`AutoExec`** (bereits in Spec 0007 beschrieben, hier nur präzisiert):
   `MessageContent::CommandResult { command, output }` — Verhalten
   unverändert, nur jetzt explizit als "danach automatisch fortsetzen"
   festgehalten.
2. **`Confirm` → Nutzer klickt "Ausführen"/"Genehmigen"** (inkl.
   `EditThenApprove`): gleiches Verhalten wie `AutoExec`, nach der
   Ausführung.
3. **`Confirm` → Nutzer klickt "Ablehnen"**: **kein** `SshTransport::execute()`-
   Aufruf, stattdessen ein neuer Varianten-Eintrag
   `MessageContent::ActionRejected { command, reason: RejectionReason::User }`
   in `context.history` — die KI erfährt explizit, dass *der Nutzer* diesen
   konkreten Vorschlag abgelehnt hat (nicht die Filter-Engine), und kann
   entsprechend reagieren (Alternative vorschlagen, nachfragen, akzeptieren
   und einen anderen Ansatz verfolgen).
4. **Filter-Engine `Deny`** (automatisch blockiert, kein Dialog, Spec 0002):
   ebenfalls `MessageContent::ActionRejected { command, reason:
   RejectionReason::Blocked(deny_reason) }` — die KI erfährt, *warum* die
   Filter-Engine blockiert hat (der `Decision::Deny`-Grund), und kann das
   berücksichtigen, statt denselben blockierten Vorschlag später erneut zu
   machen.

`ProposeNoteUpdate`-Ablehnungen folgen demselben Muster (Ergebnis-Eintrag +
automatische Fortsetzung), aber ohne eigene Sonderbehandlung — für die KI
ist eine abgelehnte Notiz-Aktualisierung inhaltlich dieselbe Art Rückmeldung
wie ein abgelehntes Kommando.

## 4. Sicherheits-Cap gegen Endlosschleifen

Eine automatische Fortsetzung, die selbst wieder eine Aktion vorschlägt,
die wieder abgelehnt/blockiert wird, könnte theoretisch endlos
weiterlaufen (Kosten- und Nutzbarkeitsrisiko). Deshalb: pro ursprünglicher
Nutzer-Nachricht ein **Zähler automatischer Fortsetzungsrunden**, Default-
Limit 10. Wird das Limit erreicht, stoppt die Automatik, eine sichtbare
Chat-Systemnachricht erscheint ("Automatische Fortsetzung nach 10 Schritten
angehalten — schreib weiter, um fortzufahren"), der Nutzer kann manuell per
neuer Nachricht weitermachen (Zähler wird dabei zurückgesetzt).

## 5. Sichtbare Kontrolle über die Automatik

Damit "automatisch weiterdenken" nicht wie ein Kontrollverlust wirkt: Sobald
eine automatische Fortsetzungsrunde läuft (KI antwortet auf ein
Aktionsergebnis, ohne dass der Nutzer getippt hat), zeigt das UI einen
dezenten, aber klar sichtbaren **"Automatik läuft" Indikator** mit einem
**"Automatik stoppen"-Button** — Klick darauf bricht die Fortsetzungskette
sofort ab (keine weiteren automatischen `send()`-Aufrufe für diese
Nutzer-Nachricht), unabhängig vom Zähler aus Abschnitt 4. Wichtig zur
Einordnung: Das stoppt nur das *automatische Weiterreden* der KI — bereits
zur Bestätigung anstehende Dialoge bleiben unabhängig davon bestehen, bis
der Nutzer sie explizit entscheidet.

## 6. Darstellung abgelehnter/blockierter Aktionen im Chatverlauf

Abgelehnte und blockierte Vorschläge verschwinden nicht aus dem Verlauf,
sondern bleiben sichtbar — durchgestrichen bzw. mit einem
"Abgelehnt"/"Blockiert"-Label, konsistent mit der bereits in Spec 0007,
Abschnitt 5 festgelegten Regel, dass auch `Deny`-Fälle transparent gezeigt
werden, nicht stillschweigend verworfen.

## 7. Bugfix: hängender Warte-Zustand

Unabhängig vom neuen Fortsetzungsverhalten: Der aktuell gemeldete
"UI bleibt blockiert"-Zustand deutet darauf hin, dass ein Warte-Flag (z. B.
"wartet auf KI-Antwort"/"Eingabe gesperrt") im Frontend nach einer Ablehnung
nicht zurückgesetzt wird. Das muss unabhängig von Abschnitt 3 korrigiert
werden — nach **jedem** der vier Ausgänge (auch ganz ohne automatische
Fortsetzung, falls Abschnitt 3 aus irgendeinem Grund nicht greift, z. B.
Netzwerkfehler beim Folge-Request) muss die Eingabe wieder nutzbar sein.
Das ist ein Fail-Safe unabhängig von der eigentlichen Fortsetzungslogik.

## 8. Offene Punkte

- Soll das Fortsetzungslimit (Abschnitt 4) in den Einstellungen konfigurierbar
  sein? Aktuell fest auf 10 — naheliegende spätere Ergänzung, aber nicht
  Teil dieser Spec.
