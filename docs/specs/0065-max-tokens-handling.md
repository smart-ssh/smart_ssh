# Spec: max_tokens-Handling (Default, Fortsetzung, Tool-Call-Schutz)

Status: Entwurf
Repo: **öffentlich** `smart_ssh`, `crates/ai-providers` (max_tokens, stop_reason-
Auswertung) + `crates/app-shell` (Orchestrierung, Fortsetzung, Retry) +
Frontend (Hinweis + „Weiter"-Knopf, Provider-Override-Feld)
Abhängigkeiten: stop_reason-Logging (0063), Rate-Limit-Gate (0061),
Prompt-Caching (0064), Filter-Engine/Confirm (0002), Auto-Fortsetzung (0021),
Provider-Formular (0056)

> **Das Problem (im echten Einsatz beobachtet, dank 0063 sichtbar):** Ein
> Request brach mit `stop_reason: max_tokens` ab — `max_tokens` stand bei
> ~4000, zu knapp für längere Skripte/Analysen. Der Nutzer sieht bestenfalls
> einen Hinweis, kann aber nichts tun.
>
> **Recherche (aktuelle Anthropic-Doku, verifiziert):** OTPM wird in Echtzeit
> auf den **tatsächlich erzeugten** Tokens gezählt — `max_tokens` fließt NICHT
> in die Rate-Limit-Berechnung ein, ein höherer Wert hat **keinen
> Rate-Limit-Nachteil**. Bezahlt werden ebenfalls nur echte Tokens. (Ältere
> Doku-Versionen sagten anderes — die aktuelle gilt.) **Andere Provider
> können abweichen** (manche Gateways reservieren anhand von `max_tokens` oder
> lehnen Werte über dem Modell-Maximum mit 400 ab).
>
> **Priorität ERHÖHT** — wegen §3: ein abgeschnittener Tool-Call darf NIE
> ausgeführt werden (Sicherheitsinvariante).

## Getroffene Entscheidungen (Stefan)

Keine reine Einstellung, kein pauschales adaptives Hochdrehen, sondern:
1. **Hoher, modellabhängiger Default** — Abschneiden wird zur Ausnahme.
2. **Abgeschnittener Text → Hinweis + „Weiter"-Knopf** (setzt fort statt neu
   zu erzeugen).
3. **Abgeschnittener Tool-Call → verwerfen + einmaliger automatischer Retry
   mit höherem Wert**, dann sichtbarer Fehler.
4. **Optionaler Override pro Provider** (für Modelle mit unbekanntem
   Output-Maximum).

## 1. Default: hoch und modellabhängig (der eigentliche Fix)

- `max_tokens` für den **Haupt-Chat** nicht mehr fest ~4000, sondern
  **modellabhängig**: orientiert am Output-Maximum des Modells, mit einem
  vernünftigen Deckel (Vorschlag **16.384** als allgemeiner Default; für
  Anthropic-Modelle darf es höher sein, z. B. 32k — kein Rate-Limit-Nachteil).
- **Modell-Maximum**: Eine kleine Lookup-Tabelle bekannter Modelle mit ihrem
  Output-Maximum (analog zur Kontextfenster-Tabelle aus 0057). **Gegen die
  tatsächlich genutzten Modelle prüfen, nicht aus dem Gedächtnis** — die
  Werte ändern sich mit Modellgenerationen. Unbekanntes Modell → konservativer
  Fallback (z. B. 8192), damit kein 400 wegen „über dem Maximum" entsteht.
- **Nebenaufrufe bleiben bewusst klein** (Zweitmeinung, Injection-Check,
  Auto-Titel, Notiz-Vorschlag, Summary): dort ist Kürze gewollt und ein
  kleiner Deckel schützt vor einem Modell, das statt „red" einen Aufsatz
  schreibt. Die aktuellen Werte dort prüfen und bewusst setzen (nicht
  versehentlich mit auf 16k ziehen).
- **Nicht-Anthropic-Provider**: Default ebenfalls modellabhängig, aber
  vorsichtiger (Gateways können reservieren/ablehnen) — §4-Override greift.

## 2. Abgeschnittener TEXT → Hinweis + „Weiter"

Wenn `stop_reason: max_tokens` (bzw. `finish_reason: length`) und die Antwort
**keinen** unvollständigen Tool-Call enthält:
- Die bis dahin erzeugte Antwort **bleibt sichtbar** (nicht verwerfen — sie
  ist gültig, nur unvollständig).
- **UI-Hinweis** an der Nachricht: „Antwort wurde abgeschnitten (Längenlimit
  erreicht)." + **„Weiter"-Knopf**.
- **„Weiter"** schickt eine Fortsetzungs-Nachricht (sinngemäß: „Deine letzte
  Antwort wurde wegen des Längenlimits abgeschnitten. Fahre exakt an der
  Stelle fort, an der sie endete, ohne zu wiederholen.") — eine **normale
  Nachricht**, funktioniert daher bei **jedem** Provider, erzeugt nur den
  fehlenden Teil neu.
- Die Fortsetzung läuft durch den **normalen Pfad** (Kompaktierung 0057,
  Gate 0061, Caching 0064, Redaction, Filter-Engine) — keine Sonderbahn.
- **Kein automatisches „Weiter"** — der Nutzer entscheidet (konsistent mit dem
  Kontroll-Prinzip; oft reicht die Teilantwort).
- **Hinweis-Text wird außerhalb jeder Untrusted-Fence gerendert** (Lehre aus
  0057: ein Hinweis *im* Fence wäre von echter Ausgabe fälschbar) — über den
  bestehenden Mechanismus (Flag/Event), nicht als Text in den Inhalt.

## 3. Abgeschnittener TOOL-CALL → nie ausführen (SICHERHEITSKRITISCH)

Schlägt `max_tokens` **mitten in einem `tool_use`-Block** zu, ist das
Kommando-JSON unvollständig — im schlimmsten Fall ein **gekürztes, aber
syntaktisch gültiges** Kommando (`rm -rf /var/log/app` statt
`rm -rf /var/log/app/old`).

- **Invariante: Ein Tool-Call aus einer Antwort mit `stop_reason:
  max_tokens` wird NIEMALS ausgeführt und NIEMALS zur Bestätigung
  vorgelegt** — auch nicht, wenn das JSON zufällig parsebar ist. Es geht nicht
  um „parsebar", sondern um „vollständig".
- **Kläre zuerst (Teil 0)**: Wie verhält sich der aktuelle Code? Wird ein
  `tool_use`-Block aus einem abgebrochenen Stream derzeit verarbeitet, wenn
  sein JSON parsebar ist? Das ist der entscheidende Ist-Befund.
- **Einmaliger automatischer Retry**: den abgeschnittenen Tool-Call verwerfen,
  die Anfrage **einmal** mit höherem `max_tokens` (verdoppelt, bis zum
  Modell-Maximum) wiederholen — unsichtbar für den Nutzer (ggf. über die
  bestehende Status-Anzeige).
- **Scheitert auch der Retry** (wieder `max_tokens`) → **sichtbarer Fehler**
  („Die KI-Antwort war zu lang für einen vollständigen Befehl"), **keine
  Schleife**, keine Ausführung. „Fehler containen".
- **Mehrere Tool-Calls in einer Antwort** (der ungetestete Multi-`tool_use`-
  Pfad aus dem Backlog): Wenn der *letzte* abgeschnitten ist, die *vorherigen*
  aber vollständig — **konservativ: die ganze Antwort als abgeschnitten
  behandeln** (keinen ihrer Tool-Calls ausführen) und retryen. Begründung:
  Die vorherigen Kommandos wurden im Kontext eines nicht zu Ende gedachten
  Plans vorgeschlagen. Beschreibe mir, falls du einen guten Grund siehst,
  davon abzuweichen.

## 4. Optionaler Override pro Provider (Experten-Feld)

- Im Provider-Formular (0056) ein **optionales** Feld „Max. Antwortlänge
  (Tokens)" mit Default **„Automatisch"**.
- Nur relevant für OpenAI-kompatible/selbstgehostete Provider, deren
  Output-Maximum die App nicht kennt (lokales Modell mit 4k-Output, Gateway
  mit eigenem Limit).
- Validierung: positive Zahl, sinnvolle Obergrenze; leer = automatisch.
- Wird der Override gesetzt, gilt er für den **Haupt-Chat** dieses Providers
  (Nebenaufrufe behalten ihre kleinen Werte).
- Visuell unauffällig (eingeklappter „Erweitert"-Bereich), damit Normalnutzer
  nicht mit einer Zahl konfrontiert werden, die sie nicht verstehen.

## Invarianten / Sicherheit
- **Abgeschnittener Tool-Call wird nie ausgeführt oder vorgelegt** (§3) —
  unabhängig von JSON-Parsebarkeit.
- Retry ist **einmalig** — keine Retry-Schleife, kein unbegrenztes Hängen.
- Fortsetzung („Weiter") läuft durch den normalen Pfad (Redaction, Fencing,
  Filter-Engine, Confirm) — keine Umgehung.
- Hinweis wird außerhalb von Untrusted-Fences gerendert (nicht fälschbar).
- Nebenaufrufe behalten kleine `max_tokens`.
- Unbekanntes Modell → konservativer Fallback (kein 400 durch zu hohen Wert).

## Testbarkeit
- **Pflicht-Regressionstest §3**: Mock-Stream, der mitten in einem
  `tool_use`-Block mit `stop_reason: max_tokens` endet — **mit parsebarem,
  aber gekürztem JSON** → Kommando wird NICHT ausgeführt/vorgelegt,
  Retry wird ausgelöst. Gegen den ungefixten Stand verifizieren (schlägt
  vorher fehl, falls der Ist-Code solche Blöcke verarbeitet).
- Retry scheitert erneut → sichtbarer Fehler, keine Schleife, keine
  Ausführung.
- Multi-`tool_use` mit abgeschnittenem letztem Block → keiner ausgeführt.
- Text-Abschnitt → Teilantwort bleibt, Hinweis + „Weiter"-Event; „Weiter"
  schickt die Fortsetzungs-Nachricht durch den normalen Pfad.
- Default-Werte: Haupt-Chat modellabhängig, unbekanntes Modell → Fallback,
  Nebenaufrufe klein.
- Override: gesetzt → gilt für Haupt-Chat; leer → automatisch.

## Reihenfolge
1. **Teil 0 + §3** (Ist-Verhalten bei abgeschnittenem Tool-Call klären, dann
   den Schutz + Retry) — sicherheitskritisch, zuerst.
2. §1 (modellabhängiger Default, Nebenaufrufe prüfen).
3. §2 (Hinweis + „Weiter").
4. §4 (Override-Feld).

## Abschluss
- `spec-reviewer` ERHÖHT (§3 adversarial: Kann ein gekürztes, parsebares
  Kommando auf irgendeinem Weg doch in Confirm/Ausführung landen?).
- CHANGELOG: „Längere KI-Antworten möglich; abgeschnittene Antworten lassen
  sich fortsetzen; abgeschnittene Befehle werden nie ausgeführt."
- Melde mir: den Ist-Befund zu §3 (wurden abgeschnittene Tool-Calls bisher
  verarbeitet?), die Modell-Maximum-Tabelle, die Nebenaufruf-Werte, und je
  Teil einen manuellen Testablauf für Stefan.
