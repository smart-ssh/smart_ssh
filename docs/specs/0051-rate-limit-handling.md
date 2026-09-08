# Spec: Rate-Limit-Handling für KI-Requests (429)

Status: Entwurf
Repo: **öffentlich** `smart_ssh`
Modul: `crates/ai-providers` (Retry/Backoff), `crates/app-shell/src/orchestration.rs`
(Request-Sequenzierung), Frontend (Fehlermeldung)
Abhängigkeiten: KI-Provider (0006), Fehler-Codes/i18n (0024/0047), Fehler-
Logging (0049 Fund 2), Redaction-Zweitmeinung (0026/0039)

> **Testphasen-Blocker.** Im echten Einsatz reproduziert: Nach wenigen
> Nachrichten "hängt" die KI. Das Log (mit dem 0049-Fehler-Logging) zeigt die
> Ursache eindeutig: **HTTP 429 `AI_RATE_LIMITED`** vom Provider — kein
> Kontext-Größen-Problem, sondern ein **fehlendes Rate-Limit-Handling**. Die
> App bekommt den 429, feuert sofort (31 ms später im Log) den nächsten
> Request, und die Sitzung bleibt stehen, statt zu warten und erneut zu
> versuchen. Betrifft **jeden Tester** (viele auf niedrigen API-Tiers mit
> strengen Limits) — nicht nur Einzelfälle.

## Teil 0 — Diagnose (erst berichten)

Bevor du fixst, kläre und berichte mir:
1. **Wie viele KI-Requests feuert die App pro Nutzer-Nachricht?** Aus dem Log
   sichtbar: mindestens ein **Redaction-Zweitmeinungs-Check** (Prompt
   "Könnte die Ausgabe … sensible Daten enthalten? none/yellow/red") **plus**
   der eigentliche Chat-Request — im Log 31 ms auseinander. Kommen weitere
   dazu (Auto-Titel, Notiz-Vorschlag)? Liste alle KI-Aufrufe pro Nachricht
   auf, mit ihrer zeitlichen Abfolge (seriell vs. quasi-gleichzeitig/Burst).
2. **Werden sie als Burst gefeuert** (mehrere quasi-gleichzeitig) oder
   seriell (einer wartet auf den anderen)? Das Log-Timing (450 ms → 481 ms)
   deutet auf einen Burst hin — bestätige oder widerlege.
3. **Was steht im 429-`Retry-After`-Header** bzw. im vollen Fehler-Body?
   Anthropic gibt oft eine konkrete Wartezeit an. Wird der Header aktuell
   gelesen? (Vermutlich nicht.)
4. **Was passiert aktuell nach dem 429** im Code — bricht die Runde ab
   (`AiEvent::Error`)? Wird im UI etwas angezeigt (vermutlich die irreführende
   "Provider-Konfiguration prüfen"-Meldung aus dem `ProviderUnavailable`/
   `RateLimited`-Mapping)? Oder hängt es sichtbar?

Berichte das, dann setze die Fixes unten um (in einem Durchgang, wenn die
Diagnose keine Überraschung bringt — sonst zurückmelden).

## Teil 1 — 429-Retry mit Backoff (Kernstück)

Ein 429 ist ein **temporärer, erwartbarer** Zustand. Statt aufzugeben:
- Bei HTTP 429 **automatisch erneut versuchen**, mit Wartezeit:
  - **`Retry-After`-Header lesen** und respektieren, falls vorhanden (das ist
    die vom Provider gewünschte Wartezeit).
  - Sonst **exponentielles Backoff** mit Jitter (z. B. 1s, 2s, 4s, …), eine
    sinnvolle **Obergrenze** an Versuchen (z. B. 3–5) und eine **Gesamt-
    Deckelung** der Wartezeit (nicht ewig retryen — nach X Sekunden aufgeben
    und Teil 3 greift).
- Gilt für **alle** KI-Requests, die 429 bekommen können — inklusive des
  **Redaction-Zweitmeinungs-Checks** (der im Log den ersten 429 bekam), nicht
  nur den Haupt-Chat-Request.
- Der Retry darf **keine** Sicherheits-Invariante verletzen: Redaction läuft
  vor jedem (Wieder-)Versand; keine unredigierten Daten durch einen Retry-
  Pfad.

## Teil 2 — Requests nicht als Burst feuern

Wenn pro Nachricht mehrere KI-Requests entstehen (Zweitmeinung + Haupt +
ggf. Titel/Notiz), diese **serialisieren/entzerren**, statt sie quasi-
gleichzeitig abzufeuern. Ein Burst reißt selbst hohe Limits kurzzeitig und
ist unnötig. Mögliche Ansätze (wähle den saubersten, begründe):
- Requests seriell nacheinander (der eine wartet aufs Ende des anderen),
  wo die Reihenfolge es ohnehin erlaubt.
- Oder eine leichte client-seitige **Rate-Begrenzung/Queue** für ausgehende
  KI-Requests (min. Abstand zwischen Requests).
- **Zusätzlich prüfen**: Muss der Redaction-Zweitmeinungs-Check bei **jedem**
  Kommando laufen? Falls er nur bei bestimmten Risiko-Fällen nötig ist, würde
  das die Request-Zahl (und damit die Rate-Last) deutlich senken — aber das
  ist eine Sicherheits-/Design-Frage (Spec 0026/0039); **nicht** eigenmächtig
  ändern, nur als Beobachtung melden, falls relevant.

## Teil 3 — Sichtbare, verständliche Fehlermeldung

Falls es nach den Retries (Teil 1) **weiter** scheitert:
- Eine **verständliche UI-Meldung** statt stillem Hängen oder dem
  irreführenden "Provider-Konfiguration prüfen": z. B. **"Der KI-Anbieter
  drosselt gerade die Anfragen (Rate Limit). Bitte kurz warten und erneut
  senden."** — mit Hinweis auf Warten, nicht auf Konfiguration.
- Dafür muss `RateLimited` (429) im UI **als eigener Fall** erkennbar sein,
  getrennt vom `ProviderUnavailable`-Sammeltopf (siehe das D2-/0006-Mapping:
  429 wird zwar als `AI_RATE_LIMITED` erkannt, aber die UI-Meldung könnte
  trotzdem generisch sein — prüfen und trennen).
- DE + EN.

## Sicherheits-/Konsistenz-Invarianten

- Redaction vor jedem (Wieder-)Versand, auch im Retry-Pfad.
- Kein Secret/Key im 429-Log (der 0049-Redaction-Pfad gilt auch hier).
- Ein Retry-Loop hat eine harte Obergrenze (Versuche **und** Gesamtzeit) — nie
  unendlich, nie stilles Hängen; nach Aufgabe greift Teil 3 (sichtbare
  Meldung).

## Testbarkeit

- Mock-Provider liefert 429 mit `Retry-After` → App wartet die angegebene
  Zeit, versucht erneut, gelingt beim zweiten Versuch → Runde läuft normal
  weiter (kein Abbruch, kein Hängen).
- Mock-Provider liefert dauerhaft 429 → App gibt nach der Obergrenze auf und
  zeigt die verständliche Rate-Limit-Meldung (kein stilles Hängen).
- Burst-Test: mehrere KI-Requests pro Nachricht werden nicht quasi-
  gleichzeitig abgefeuert (nachweisbar entzerrt/serialisiert).
- Regressionstest gegen den ungefixten Stand (429 → sofortiger Abbruch/Hängen).

## Hinweis

Dieser Fix ist **unabhängig** vom Kontext-Größen-Thema (ungekürzte
Server-Notiz, eigener Backlog-Eintrag/Spec) — beide verschärfen sich
gegenseitig (große Notiz = mehr Tokens pro Request = schneller ans Token-
Rate-Limit), aber das Rate-Limit-Handling ist der akute Testphasen-Blocker
und wird zuerst gelöst. Die Notiz-Kürzung folgt separat.
