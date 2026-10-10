# Spec 0087 — Budget- und Anfrage-Randfälle der KI-Anbieter

Status: umgesetzt
Zweck: Ein OpenAI-kompatibler Server mit kleinem Kontext liefert eine
Antwort statt HTTP 400; kein Retry unterschreitet eine explizite
Nutzereinstellung; eine leere System-Nachricht geht nicht mehr hinaus.
Bezüge: Spec 0065 (Längenlimit, abgeschnittene Antworten), Spec 0080
(Budget-Defaults, leere Runde), Spec 0081 (leerer System-Block bei
Anthropic), Spec 0072 (Fehlerabbildung), Spec 0049 (Redaction der Logs).
Review-Einstufung: normal.

## 1. Ausgangslage

**Retry beim Abschneiden.** Beide Anbieter wiederholen eine abgeschnittene
Runde einmal mit verdoppeltem Längenlimit. Beim OpenAI-kompatiblen Anbieter
unterschreitet der Retry ein explizites Override nie; beim Anthropic-Anbieter
konnte er es unterschreiten (Override über dem Modell-Maximum).

**Kontextgrenzen.** Ein 400 ohne Modell-Merkmal wird als „Provider nicht
erreichbar" gemeldet. Die Fehlerart „Kontext zu groß" existiert, wurde aber
von keinem Anbieter erzeugt. Die Server verhalten sich verschieden:

- vLLM: Limit größer als Kontext → HTTP 400 `max_tokens=8192 cannot be
  greater than max_model_len=4096…`; Eingabe plus Limit größer als Kontext
  → HTTP 400 `This model's maximum context length is 4096 tokens. However,
  you requested …`.
- OpenRouter: `This endpoint's maximum context length is N tokens. However,
  you requested about M tokens …` (nach Berichten Dritter, nicht gemessen).
- llama.cpp (gemessen): Ein zu großes Limit bei kurzem Prompt ergibt
  **HTTP 200** mit normaler Antwort; nur eine zu lange Eingabe ergibt
  HTTP 400 (`exceed_context_size_error`). Ein kleineres Limit hilft dort
  nicht.

Der Standard für unbekannte Modelle an fremden Endpunkten (Spec 0080)
trifft damit jeden vLLM-Server mit höchstens 8192 Token Kontext schon bei
der ersten Anfrage.

**Leere System-Nachricht.** Der OpenAI-kompatible Anbieter sendete immer
eine System-Nachricht, auch mit leerem Text (nur bei nativem Tool-Calling
und leerem Kontext, praktisch der Zugangsdaten-Test). llama.cpp nimmt sie an
(gemessen); die übrigen Server sind nicht gemessen. Das ist eine
Angleichung an Anthropic (Spec 0081), kein belegter Fehler.

Das Rate-Limit-Budget (Spec 0061) wird beim OpenAI-kompatiblen Anbieter
nicht beschrieben; ein zusätzlicher Request dort umgeht keine Sperre.

## 2. Ziel und Nicht-Ziele

Ziel: A1–A3.

Nicht-Ziele:

- Kein Retry bei llama.cpps `exceed_context_size_error`; der Fehler bleibt
  „Provider nicht erreichbar" (Eingabe zu lang).
- Keine Kontextgrenzen-Erkennung beim Anthropic-Anbieter (die Maxima sind
  dort bekannt).
- Kein Rechnen mit den Zahlen aus dem Fehlertext; keine Änderung der
  Defaults aus Spec 0080.
- Keine Änderung an Kompaktierung, Verlauf oder Einstellungsformular.

## 3. Anforderungen

**A1 — Kontextgrenze (OpenAI-kompatibel)**

- A1.1 MUSS: Eine HTTP-400-Antwort, deren Körper (ohne Groß-/Kleinschreibung)
  `maximum context length is` oder `cannot be greater than max_model_len`
  enthält, gilt als Kontextgrenzen-Fehler. Andere Statuscodes und andere
  400-Körper (insbesondere llama.cpps `exceeds the available context size`)
  bleiben wie zuvor. Ein 400, der als „Modell nicht gefunden" erkannt wird,
  bleibt „Modell nicht gefunden". Die Erkennung wirkt nur beim
  OpenAI-kompatiblen Anbieter; Anthropic und die Modell-Ermittlung bilden
  Fehler weiter wie zuvor ab.
- A1.2 MUSS: Bei einem Kontextgrenzen-Fehler wiederholt der Anbieter die
  Anfrage **einmal** mit einem Limit von 2048 (gleicher Feldname wie in der
  ersten Anfrage, `max_tokens` bzw. `max_completion_tokens`) — aber nur,
  wenn (a) das gesendete Limit **nicht** aus einem vom Nutzer gesetzten
  Override stammt, (b) 2048 kleiner als das gesendete Limit ist und (c) in
  diesem Aufruf noch kein Abschneide-Retry lief (A1.4). Sonst kein Retry.
  Aus dem Override stammt das Limit genau dann, wenn kein Hint eines
  Nebenaufrufs gesetzt und der Override gesetzt ist; ein solcher Hint gilt
  nie als Nutzereinstellung.
- A1.3 MUSS: Kommt kein Retry zustande oder scheitert auch der Retry mit
  einem Kontextgrenzen-Fehler, endet die Runde mit „Kontext zu groß" — kein
  dritter Versuch, kein Hängen. Ausnahme: A1.4, zweiter Punkt.
- A1.4 MUSS: Je Aufruf läuft höchstens **einer** der beiden Retrys
  (Kontext-Retry nach A1.2 oder Abschneide-Retry), also höchstens zwei
  HTTP-Anfragen (429-Wiederholungen nicht mitgezählt):
  - nach einem Kontext-Retry endet eine Antwort, die sonst den
    Abschneide-Retry auslösen würde (abgeschnittener Kommandovorschlag,
    leere `length`-Runde), direkt mit „Antwort abgeschnitten", ohne weitere
    Anfrage;
  - nach einem Abschneide-Retry endet ein Kontextgrenzen-Fehler direkt mit
    „Antwort abgeschnitten" (die Antwort passt nicht ins Limit; der Hinweis
    von „Kontext zu groß" wäre dort der falsche Rat).
- A1.5 MUSS: Der Text zu `AI_CONTEXT_TOO_LARGE` (DE/EN) nennt die Abhilfe:
  die Einstellung „Max. Antwortlänge (Tokens)" / „Max. response length
  (tokens)" niedriger setzen oder einen neuen Chat beginnen.
- A1.6 MUSS: Der ursprüngliche 400-Körper wird vor dem Retry über den
  redigierenden Fehler-Logpfad geloggt wie jeder Anbieterfehler; ein
  erfolgreicher Retry erzeugt eine Debug-Zeile mit altem und neuem Limit,
  ohne Körper.

**A2 — Anthropic-Retry respektiert den Override**

- A2.1 MUSS: Der Abschneide-Retry des Anthropic-Anbieters sendet nie weniger
  als das Limit der ersten Anfrage — gleiche Regel wie beim
  OpenAI-kompatiblen Anbieter.

**A3 — Keine leere System-Nachricht (OpenAI-kompatibel)**

- A3.1 MUSS: Ist der System-Text nach dem Fallback-Zusatz leer oder besteht
  nur aus Leerraum (wie bei Anthropic), enthält die Nachrichtenliste keine
  System-Nachricht. Im Fallback-Modus ist er nie leer und bleibt
  unverändert.

## 5. Design

- Festes Retry-Limit 2048 statt Rechnen aus dem Fehlertext: drei
  Textformate mit Zahlen in Prosa wären fragil, und Halbieren (8192 → 4096)
  scheitert bei genau 4k-Kontext weiterhin.
- „Kontext zu groß" bleibt eine Einheitsvariante (kein neuer Code); der
  Zugangsdaten-Test ordnet sie wie bisher als „nicht erreichbar" mit Code
  ein.

## 6. Sicherheitszusagen

- **Abgeschnittene Antworten** (Spec 0065): Ein abgeschnittener
  Kommandovorschlag wird nie ausgeführt oder vorgelegt. A1.4 ändert nur, ob
  vor „Antwort abgeschnitten" noch ein Versuch läuft, nie, was mit dem
  abgeschnittenen Inhalt geschieht.
- **Redaction der Fehlerlogs** (Spec 0049): Der Pfad loggt den 400 über
  dieselbe Funktion wie jeder Fehler (API-Key und Zusatz-Header redigiert);
  die Debug-Zeile aus A1.6 enthält nur Zahlen.
- **Nie hängen:** höchstens ein Kontext-Retry, Zeitgrenzen wie beim ersten
  Versuch.
- Filter, Risiko, Bestätigung und Credentials sind nicht berührt.

## 7. Akzeptanzfälle

- T1 (A1.2) vLLM-Text „`max_tokens=8192 cannot be greater than
  max_model_len=4096…`", 400, danach 200: Der Nutzer erhält die Antwort; die
  zweite Anfrage trägt 2048, die erste 8192.
- T2 (A1.1) wie T1 mit „`This model's maximum context length is 4096
  tokens. However, you requested …`"; T2b mit OpenRouter-Text in
  Kleinschreibung. Beide: Retry mit 2048.
- T3 (A1.1, negativ) llama.cpp-Körper: genau **eine** Anfrage, „Provider
  nicht erreichbar".
- T4 (A1.1, negativ) 400 mit Modell-nicht-gefunden-Körper, der zusätzlich
  „maximum context length is" enthält: „Modell nicht gefunden", eine
  Anfrage.
- T5 (A1.1, negativ) 413/500 mit Kontextgrenzen-Text: kein Retry.
- T6 (A1.2a) Override 8192 gesetzt, 400 mit vLLM-Text: genau eine Anfrage,
  „Kontext zu groß".
- T7 (A1.2b) Nebenaufruf mit Hint ≤ 2048, 400 mit vLLM-Text: eine Anfrage,
  „Kontext zu groß".
- T7b (A1.2a) Hint 4096 **und** Override 8192, 400 mit vLLM-Text, dann 200:
  Retry mit 2048, Antwort kommt an (der Hint ist keine Nutzereinstellung).
- T8 (A1.3) zweimal 400 mit Kontexttext: genau zwei Anfragen, „Kontext zu
  groß".
- T9 (A1.4) 400 mit Kontexttext, dann 200 mit `length` ohne Inhalt: genau
  zwei Anfragen, „Antwort abgeschnitten"; kein Limit über 2048.
- T9b (A1.4) 200 mit `length` ohne Inhalt, dann 400 mit Kontexttext: genau
  zwei Anfragen, „Antwort abgeschnitten", keine Anfrage mit 2048.
- T10 (A1.2) Reasoning-Modell (`max_completion_tokens`): Der Retry setzt
  dasselbe Feld, das andere fehlt.
- T11 (A1.6) Vor dem Retry existiert eine Fehler-Logzeile mit Status 400, der
  eingebettete API-Key erscheint nur redigiert; nach erfolgreichem Retry
  eine Debug-Zeile mit 8192 und 2048.
- T11b (A1.1, negativ) Anthropic, 400 mit „maximum context length is": eine
  Anfrage, „Provider nicht erreichbar".
- T12 (A1.5) Die DE- und EN-Texte enthalten das jeweilige Label der
  Override-Einstellung (aus den Sprachdateien gelesen); T12b: Der deutsche
  Fehlertext enthält „Max. Antwortlänge".
- T13 (A2.1) Anthropic, Override über dem Modell-Maximum, abgeschnittene
  Runde: Der Retry trägt mindestens den Override.
- T14 (A2.1) Anthropic ohne Override: Verdopplung bis zum Modell-Maximum wie
  zuvor.
- T15 (A3.1) Natives Tool-Calling, leerer System-Text (`""` und `"  \n"`):
  Die erste Nachricht ist die Nutzernachricht.
- T16 (A3.1) Fallback-Modus, leerer Kontext: System-Nachricht mit
  Fallback-Zusatz vorhanden.

Manueller Test: gegen einen vLLM-Server mit `--max-model-len 4096` eine
Nachricht senden; es kommt eine Antwort, kein HTTP 400.
