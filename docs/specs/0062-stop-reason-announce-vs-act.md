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
einem Kommando bleiben ausdrücklich erlaubt. Der System-Prompt folgt der
UI-Sprache (Deutsch oder Englisch, Spec 0024); beide Fassungen tragen
denselben Inhalt.

## Teil 3: Unabhängige Kommandos bündeln, nicht-interaktiv ausführen

Damit eine Aufgabe weniger KI-Anfragen braucht, weist der System-Prompt in
beiden Sprachen die KI an:

- Braucht sie mehrere Informationen oder Schritte, die **nicht** vom
  Ergebnis des jeweils anderen abhängen, schlägt sie diese als getrennte
  Kommando-Vorschläge in **derselben** Antwort vor.
- Schritte, die von einem früheren Ergebnis abhängen, bleiben in
  getrennten Runden.
- Der Nutzer bestätigt oder lehnt jeden Vorschlag einzeln ab; die KI
  bekommt danach alle Ergebnisse, auch die Ablehnungen, gesammelt in einer
  Folgeanfrage.
- Sie fasst unabhängige Kommandos nicht mit `&&` oder `;` zu einer
  Kommandozeile zusammen, nur um eine Runde zu sparen. Getrennte Vorschläge
  halten jedes Kommando einzeln prüfbar.
- Würde ein Kommando eine Rückfrage stellen, nutzt sie die
  nicht-interaktive Variante (z. B. `-y`/`--yes`,
  `DEBIAN_FRONTEND=noninteractive` für apt, `--non-interactive` oder
  `--noconfirm`, wo vorhanden). Eine Rückfrage während der Ausführung kann
  die KI nicht beantworten; das Kommando würde bis zum Abbruch durch den
  Nutzer warten.

Die Werkzeug-Beschreibung für Kommando-Vorschläge beschreibt ein Kommando
je Aufruf und erlaubt mehrere Aufrufe je Antwort. Sie steht in derselben
Sprache wie der System-Prompt. Eine Einstellung dafür gibt es nicht.

## Sicherheitszusagen

- Das Logging des Abbruchgrunds enthält keinen sensiblen Inhalt, nur das
  Grund-Feld und die Request-Kennung.
- Der Prompt ändert nichts an Filter und Bestätigung: Ein vorgeschlagenes
  Kommando läuft unverändert durch die Filter-Engine und die Bestätigung.
  Der Prompt beeinflusst nur, *ob* die KI das Tool nutzt, nicht, was danach
  geschieht.
- Gebündelte Vorschläge laufen einzeln und in der vorgeschlagenen
  Reihenfolge durch Filter, Risiko-Einstufung und Bestätigung. Nach einer
  Ablehnung braucht jeder weitere Vorschlag derselben Antwort eine
  Bestätigung (ADR 0059); daran ändert das Bündeln nichts.

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
- Der System-Prompt enthält in beiden Sprachen die Bündel-Anweisung
  (unabhängige Schritte in einer Antwort, abhängige in getrennten Runden,
  kein Verketten mit `&&`/`;` nur um eine Runde zu sparen) und die
  Anweisung zu nicht-interaktiven Varianten mit mindestens `-y`/`--yes`
  und `DEBIAN_FRONTEND=noninteractive`.
- Die Werkzeug-Beschreibung für Kommando-Vorschläge erlaubt mehrere
  Aufrufe je Antwort und steht in der Sprache des System-Prompts.
