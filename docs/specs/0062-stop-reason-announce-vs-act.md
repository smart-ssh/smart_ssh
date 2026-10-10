# Spec 0062 — Abbruchgrund sichtbar, „handeln statt ankündigen"

Status: umgesetzt
Zweck: Endet ein KI-Turn, ohne dass die KI ein angekündigtes Kommando
tatsächlich vorschlägt, lässt sich der Grund erkennen, und die KI wird
angehalten, Kommandos auszuführen statt nur anzukündigen.
Bezüge: Spec 0006 (Anbieter), Spec 0021 (Fortsetzung), Spec 0007.

## Problem

Die KI schreibt gelegentlich eine **Ankündigung** („Lassen wir uns Status
und Journal anzeigen:") und beendet den Turn, ohne ein Kommando
vorzuschlagen. Ohne Vorschlag gibt es nichts, worauf die automatische
Fortsetzung (Spec 0021) reagieren könnte; sie stoppt. Für den Nutzer wirkt
es, als bliebe die KI mitten im Satz stehen.

## Teil 1: Abbruchgrund erfassen

Der Grund, warum eine Antwort endete, wird gelesen und im Log festgehalten
(zusammen mit der Request-Kennung, ohne sensiblen Inhalt):

- Anthropic: `stop_reason` aus dem Ende-Ereignis des Streams.
- OpenAI-kompatibel: `finish_reason` der letzten Antwortstückes.

So lässt sich unterscheiden, ob das Modell **bewusst** aufhörte (`end_turn`/
`stop`), ob die Antwort **technisch abgeschnitten** wurde (`max_tokens`/
`length`; dann ist das Limit zu niedrig, siehe Spec 0065) oder ob ein
Tool-Aufruf folgt.

## Teil 2: System-Prompt „handeln statt ankündigen"

Der System-Prompt der Sitzung weist die KI an, ein Kommando **auszuführen**
(das Tool aufzurufen), statt es nur anzukündigen. Kurze Erklärungen vor
einem Kommando bleiben ausdrücklich erlaubt. Es gibt einen einzigen,
deutschen System-Prompt; die zweisprachigen Oberflächentexte sind davon
getrennt.

## Sicherheitszusagen

- Das Logging des Abbruchgrunds enthält keinen sensiblen Inhalt, nur das
  Grund-Feld und die Request-Kennung.
- Der Prompt ändert nichts an Filter und Bestätigung: Ein vorgeschlagenes
  Kommando läuft unverändert durch die Filter-Engine und die Bestätigung.
  Der Prompt beeinflusst nur, *ob* die KI das Tool nutzt, nicht, was danach
  geschieht.

## Grenzen

- Ein automatisches Nachhaken der App bei einer Ankündigung ohne
  Vorschlag gibt es nicht.
- Ob die Prompt-Ergänzung das reale Modellverhalten ändert, ist nicht
  automatisiert prüfbar.

## Akzeptanzfälle

- `end_turn`/`stop` und `max_tokens`/`length` werden für beide Anbieter
  über den echten Stream-Pfad erkannt und geloggt.
- Der Prompt enthält die „handeln statt ankündigen"-Anweisung und behält
  die Erlaubnis zu kurzen Erklärungen.
