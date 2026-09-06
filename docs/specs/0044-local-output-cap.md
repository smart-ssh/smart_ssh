# Spec: Output-Cap für den lokalen Pseudo-Server

Status: Entwurf
Modul: `crates/ssh-transport` (`LocalTransport`)
Abhängigkeiten: Ressourcen-Caps (0043, Fund A — etabliert das Muster),
lokaler Pseudo-Server (0032)

> **Nummerierung**: nächste freie Nummer in deiner Reihe. Kleiner
> Nachzieher zu Spec 0043 — schließt die letzte offene Stelle derselben
> Cap-Klasse.

## 1. Problem

Spec 0043, Fund A hat den Output-Cap für den **Remote**-SSH-Pfad
(`RusshTransport::execute`) so umgebaut, dass er **während** des Streamings
greift statt nach vollständigem Puffern — ein feindlicher Server kann so
den Speicher nicht mehr vor dem Cap erschöpfen. Der **lokale Pseudo-Server**
(`LocalTransport::execute`, Spec 0032) blieb dabei bewusst außerhalb des
Scopes (0043 zielte explizit auf den Remote-Pfad) und hat weiterhin
**unbegrenzte Ausgabe**: Ein lokales Kommando, das sehr viel ausgibt (z. B.
`cat` einer riesigen Datei, `yes`), puffert unbegrenzt.

Das Risikoprofil ist geringer als beim Remote-Fall (der lokale
Pseudo-Server führt Kommandos auf der eigenen Maschine des Nutzers aus, kein
externer Angreifer steuert die Ausgabe) — aber es ist dieselbe
Erschöpfungsklasse, und ein von der KI vorgeschlagenes, versehentlich
ausuferndes lokales Kommando (oder eine fehlerhafte Schleife) kann die App
zum Speicherüberlauf bringen. Konsistenz mit dem Remote-Pfad ist hier das
Hauptargument.

## 2. Fix

Wende denselben Streaming-Cap-Mechanismus aus 0043 Fund A auf
`LocalTransport::execute` an:

- Bytes werden **während** des Lesens der lokalen Prozess-Ausgabe gezählt;
  bei Erreichen des Limits (derselbe konfigurierbare Default wie Remote,
  2 MB) wird das Lesen abgebrochen, der Rest verworfen, der Prozess
  ggf. beendet.
- Das Ergebnis wird als abgeschnitten markiert (`truncated: true`),
  identisch zum Remote-Pfad — UI und KI-Kontext machen es genauso kenntlich.
- **Kein zweiter Mechanismus**: Wenn die Cap-Logik aus 0043 als
  wiederverwendbare Funktion/Helfer vorliegt, wird sie geteilt, nicht
  dupliziert. Falls sie aktuell in `RusshTransport` eingebettet ist, an
  eine gemeinsam nutzbare Stelle ziehen (kleines Refactoring), sodass beide
  Transporte denselben Code nutzen.

## 3. Nicht-Ziele

- Der interaktive lokale PTY-Modus (`LocalTransport::open_shell`) ist wie
  beim Remote-Fall nicht betroffen — er streamt fortlaufend ins Terminal,
  xterm.js cappt den Scrollback (in 0043 verifiziert). Nur der
  `execute`-Pfad (Exec-Modus) bekommt den Cap.

## 4. Testbarkeit

- Ein lokales Kommando, das mehr als das (im Test kleiner gesetzte) Limit
  ausgibt, belegt: Puffer wächst nicht über das Limit, Ergebnis ist als
  abgeschnitten markiert — analog zum Remote-Test aus 0043, aber gegen
  `LocalTransport`.
- Falls die Cap-Logik geteilt wird: ein Test, der belegt, dass beide
  Transporte dieselbe Grenze anwenden (kein Auseinanderdriften der
  Defaults).

## 5. Sicherheits-Invariante

- Der Cap markiert nur als abgeschnitten, schwächt keine
  Sicherheitsentscheidung ab (identisch zu 0043 Fund A).
