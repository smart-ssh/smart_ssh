# Spec 0065 — Umgang mit der maximalen Antwortlänge (max_tokens)

Status: umgesetzt
Zweck: Antworten werden nicht durch ein zu knappes Längenlimit abgeschnitten;
wenn es doch passiert, bleibt die Teilantwort nutzbar, und ein
abgeschnittener Kommandovorschlag wird nie ausgeführt.
Bezüge: Spec 0062 (Abbruchgrund), Spec 0061/0064 (Rate-Limit, Cache),
Spec 0002 (Filter-Engine), Spec 0021 (Fortsetzung), Spec 0056
(Anbieter-Formular), Spec 0087 (Kontextgrenzen-Fehler).

## Ausgangslage

Ein zu niedriges Längenlimit ließ Antworten bei längeren Skripten oder
Analysen mit Abbruchgrund `max_tokens` enden. Bei Anthropic zählt das
Output-Limit auf tatsächlich erzeugten Tokens; ein höherer Wert hat keinen
Rate-Limit-Nachteil und keine höheren Kosten. Andere Anbieter können
abweichen (Gateways reservieren anhand des Werts oder lehnen Werte über dem
Modell-Maximum mit 400 ab).

## 1. Hoher, modellabhängiger Standard

- Das Limit für den **Haupt-Chat** orientiert sich am bekannten
  Output-Maximum des Modells: bei Anthropic die Hälfte davon, bei sehr
  großen Maxima (128k) 32 000, bei 64k-Modellen 16 384. Eine kleine Tabelle
  bekannter Modelle liefert die Maxima; sie wird gegen die tatsächlich
  genutzten Modelle gepflegt.
- Ein unbekanntes Modell bekommt ein konservatives Fallback-Maximum (Standard: die Hälfte davon), damit kein
  400 wegen „über dem Maximum" entsteht (Anthropic 8192; offizielle OpenAI-API
  4096; andere OpenAI-kompatible Endpunkte 16 384, Standard dort 8192).
- **Nebenaufrufe bleiben klein** (Zweitmeinung, Einschleusungs-Prüfung,
  Auto-Titel, Notiz-Vorschlag, Zusammenfassung): Kürze ist gewollt, und ein
  kleiner Deckel schützt vor einem Modell, das statt „red" einen Aufsatz
  schreibt. Der Deckel liegt bei 4096.

## 2. Abgeschnittener Text → Hinweis und „Weiter"

Endet eine Antwort wegen des Längenlimits und enthält **keinen**
unvollständigen Kommandovorschlag:

- Die bis dahin erzeugte Antwort bleibt sichtbar (sie ist gültig, nur
  unvollständig).
- An der Nachricht erscheint der Hinweis „Antwort wurde abgeschnitten
  (Längenlimit erreicht)." mit einem **„Weiter"-Knopf**.
- „Weiter" schickt eine normale Fortsetzungs-Nachricht (sinngemäß: die
  letzte Antwort wurde abgeschnitten, fahre exakt dort fort, ohne zu
  wiederholen). Das funktioniert bei jedem Anbieter und erzeugt nur den
  fehlenden Teil.
- Die Fortsetzung läuft durch den normalen Pfad (Kompaktierung, Drosselung,
  Caching, Redaction, Filter-Engine), keine Sonderbahn.
- Es gibt **kein automatisches „Weiter"**; der Nutzer entscheidet.
- Der Hinweis wird außerhalb jeder Untrusted-Fence gerendert, damit echte
  Ausgabe ihn nicht fälschen kann.

## 3. Abgeschnittener Kommandovorschlag → nie ausführen

Schlägt das Limit mitten in einem Kommandovorschlag zu, ist dieser im
schlimmsten Fall ein gekürztes, aber syntaktisch gültiges Kommando
(`rm -rf /var/log/app` statt `rm -rf /var/log/app/old`).

- **Ein Vorschlag aus einer wegen des Längenlimits beendeten Antwort wird
  niemals ausgeführt und niemals zur Bestätigung vorgelegt**, auch wenn
  sein JSON zufällig parsebar ist. Maßgeblich ist „vollständig", nicht
  „parsebar".
- Die App verwirft den Vorschlag und wiederholt die Anfrage **einmal**
  automatisch mit einem höheren Limit (verdoppelt, bis zum Modell-Maximum),
  unsichtbar für den Nutzer.
- Scheitert auch der Retry mit demselben Grund, erscheint ein sichtbarer
  Fehler („Die KI-Antwort war zu lang für einen vollständigen Befehl"), ohne
  Schleife und ohne Ausführung.
- Enthält eine Antwort mehrere Vorschläge und ist der letzte abgeschnitten,
  gilt die **ganze Antwort** als abgeschnitten: keiner der Vorschläge wird
  ausgeführt, die Anfrage wird wiederholt. Die vorherigen Vorschläge
  entstanden im Rahmen eines nicht zu Ende gedachten Plans.
- Zusammenspiel mit einem Kontextgrenzen-Fehler (Spec 0087): je Aufruf läuft
  höchstens einer der beiden Retrys.

## 4. Optionaler Override je Anbieter

Das Anbieter-Formular (Spec 0056) bietet im eingeklappten
„Erweitert"-Bereich ein optionales Feld „Max. Antwortlänge (Tokens)" mit
Standard „Automatisch". Es ist für OpenAI-kompatible und selbstgehostete
Anbieter gedacht, deren Output-Maximum die App nicht kennt.

- Gültig ist eine positive Zahl mit sinnvoller Obergrenze; leer heißt
  automatisch.
- Ein gesetzter Wert gilt für den Haupt-Chat dieses Anbieters; Nebenaufrufe
  behalten ihre kleinen Werte.
- Ein Retry nach einem Kontextgrenzen-Fehler überschreibt einen vom Nutzer
  gesetzten Wert nicht (Spec 0087).

## Sicherheitszusagen

- Ein abgeschnittener Vorschlag wird nie ausgeführt oder vorgelegt (3).
- Der Retry ist einmalig: keine Schleife, kein unbegrenztes Hängen.
- „Weiter" läuft durch Redaction, Fencing, Filter-Engine und Bestätigung.
- Der Hinweis lässt sich nicht durch Ausgabeinhalt fälschen.
- Ein unbekanntes Modell führt nicht zu einem 400 durch einen zu hohen Wert.

## Akzeptanzfälle

- Ein Stream, der mitten im Kommandovorschlag mit `max_tokens` endet, auch
  mit parsebarem gekürztem JSON, wird weder ausgeführt noch vorgelegt; ein
  Retry läuft.
- Scheitert der Retry erneut: sichtbarer Fehler, keine Schleife, keine
  Ausführung.
- Mehrere Vorschläge mit abgeschnittenem letztem: keiner wird ausgeführt.
- Abgeschnittener Text: Teilantwort bleibt, Hinweis und „Weiter" erscheinen,
  „Weiter" geht durch den normalen Pfad.
- Standardwerte: Haupt-Chat modellabhängig, unbekanntes Modell Fallback,
  Nebenaufrufe klein; Override gilt für den Haupt-Chat.
