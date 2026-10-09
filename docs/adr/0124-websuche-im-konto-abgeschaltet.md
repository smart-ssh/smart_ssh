# ADR 0124 — Websuche im Provider-Konto abgeschaltet: einmal ohne Web-Werkzeuge wiederholen

Status: akzeptiert
Betrifft: Issue #169, Spec 0105 (§3, §7), Spec 0051, Spec 0061, Spec 0064

## Problem

Die Web-Recherche (Spec 0105) ist je Provider standardmäßig an. Hat der
Betreiber eines Anthropic-Kontos die Websuche abgeschaltet, lehnt der
Provider jede Anfrage mit Web-Werkzeug ab. Ohne Gegenmaßnahme scheitert
damit jede Chat-Nachricht, bis der Nutzer die Einstellung findet und
ausschaltet.

## Entscheidung

1. **Erkennung nur an der belegten Fehlerform.** Laut Anthropic-
   Dokumentation zum Web-Search-Werkzeug scheitert eine Anfrage mit dem
   Werkzeug bei abgeschalteter Websuche mit HTTP 400
   `invalid_request_error` und einer Meldung, dass die Websuche nicht
   aktiviert ist („web search is not enabled"), nicht mit einem Fehlercode
   im Suchergebnis. Die App verlangt alle drei Merkmale zusammen: Status
   400, `error.type == "invalid_request_error"` (erste Ebene unter `error`)
   und eine Meldung, die ein Web-Werkzeug nennt („web search", „web_search",
   „web fetch", „web_fetch") **und** „not enabled" enthält
   (Groß-/Kleinschreibung egal). Text allein, ein anderer Fehlertyp oder ein
   anderer Status zählen nicht.
2. **Nur für Anfragen mit Web-Werkzeug.** Der Provider meldet den neuen
   Fehler `AiError::WebResearchRejected` nur, wenn der abgelehnte
   Request-Body tatsächlich `web_search` oder `web_fetch` enthielt. Ein
   Request ohne Web-Werkzeug (der Wiederholversuch, jeder Nebenaufruf) kann
   ihn nie auslösen.
3. **Wiederholt wird im Chat-Turn, nicht im Provider.** Anders als der
   429-Backoff (Spec 0051) und der `max_tokens`-Retry (Spec 0065), die im
   Provider liegen, wiederholt `app-logic` die Anfrage. Nur dort laufen
   Mindestabstand (`wait_for_ai_request_slot`) und proaktives
   Rate-Limit-Gate (`wait_for_rate_limit_budget`, Spec 0061). Das Issue
   verlangt, dass der Wiederholversuch dieselbe Budget- und
   Rate-Abrechnung durchläuft wie eine normale Anfrage. Der zweite Request
   nutzt denselben, schon kompaktierten und geschwärzten Kontext. Der
   Provider liest auch bei ihm die Rate-Limit-Header wie bei jeder Antwort.
4. **Merken für die Sitzung, nur im Speicher (Option b aus dem Issue).**
   Neue Trait-Methode `AiProvider::disable_web_research()` (Default: nichts).
   `AnthropicProvider` setzt damit ein Flag, ab dem `build_request_body`
   keine Web-Werkzeuge mehr anhängt. Die Provider-Instanz entsteht je
   Sitzung beim Verbinden, das Flag lebt also genau eine Sitzung. Die
   gespeicherte Einstellung wird nicht geändert (Option c verworfen: eine
   still geänderte Nutzereinstellung). Option a (bei jeder Nachricht erneut
   ein gescheiterter Request) verworfen: zusätzliche Wartezeit und
   Anfragen ohne Nutzen.
5. **Höchstens ein Versuch.** Ein lokaler Merker in `run_one_round` lässt
   je Anfrage nur einen Wiederholversuch zu. Meldet auch der zweite Request
   einen Fehler, gleich welchen, erscheint er als normale Fehlerkarte.
6. **Hinweis als eigenes Event.** `chat-web-research-unavailable` trägt nur
   die Sitzungs-ID. Der Hinweistext ist ein fester, übersetzter Text mit
   Verweis auf die Einstellung, ohne den Fehlertext des Providers. Er wird
   wie die übrigen Chat-Hinweise (z. B. „Antwort abgebrochen") nicht im
   Verlauf gespeichert.

## Konsequenzen

- Ein betroffener Nutzer bekommt eine Antwort statt eines Fehlers. Je
  Sitzung kostet das einen abgelehnten Request und einen Hinweis.
- Der Werkzeugsatz einer Sitzung kann sich einmal ändern. Damit verfällt
  der Cache-Eintrag für die Werkzeuge (Spec 0064) einmalig. Danach ist der
  Satz wieder konstant.
- Ändert Anthropic die Fehlermeldung so, dass sie die Merkmale aus Punkt 1
  nicht mehr trifft, fällt die App auf das alte Verhalten zurück
  (Fehlerkarte; Abhilfe Einstellung ausschalten). Es gibt dann keinen
  falschen Wiederholversuch.
- Ob ein Konto auch nur `web_fetch` separat abschalten kann, ist nicht
  dokumentiert. Die Erkennung schließt eine entsprechend formulierte
  Meldung ein, weil die Abhilfe (beide Web-Werkzeuge weglassen) dieselbe
  ist.
- Sicherheit: Der Wiederholversuch entfernt nur Werkzeuge und fügt keine
  hinzu. Er sendet denselben, bereits geschwärzten Kontext. An Filter,
  Risiko-Einstufung und Bestätigung ändert sich nichts.
