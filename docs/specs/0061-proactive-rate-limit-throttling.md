# Spec: Proaktive, provider-weite Rate-Limit-Drosselung

Status: Entwurf
Repo: **öffentlich** `smart_ssh`, `crates/ai-providers` (Header-Auswertung,
Budget-Wächter) + `crates/app-shell` (Aufruf-Gate) + Frontend (Warteanzeige)
Abhängigkeiten: Rate-Limit-Retry (0051), Body-Timeout-Fix, die Diagnose
(claude-code-prompt-diagnose-rate-limit-ursache), Session-Modell (0057),
Recherche (Anthropic-Doku + SRE-Best-Practices)

> **Das Problem (aus der Diagnose bestätigt):** Die App fliegt **blind** ins
> Rate-Limit — die `anthropic-ratelimit-*`-Header werden **gar nicht** gelesen,
> die App merkt erst *nach* dem 429, dass es eng war. Und die Kompaktierung
> (0057) schützt nur vor dem **Kontextfenster** (150k Tokens), NICHT vor dem
> **Rate-Limit** (Input-TPM, auf niedrigen Tarifen 20–40k Tokens/Minute — 4–7×
> kleiner). Das ist eine **andere Achse**. Mehrere kleine Requests/Minute
> sprengen das TPM-Budget, ohne je die Kompaktierung auszulösen.
>
> **Die Lösung:** Header lesen → **geteiltes Budget pro Provider-Identität** →
> vor jedem Send prüfen → bei knappem Budget **warten** (statt feuern) → UI
> zeigt das Warten. **Priorität ERHÖHT** (KI-Request-Pfad, den 0051/
> Body-Timeout/0057 schon härteten — Invarianten beachten).

## Getroffene Entscheidungen (Stefan)

1. **Konservativ drosseln**: schon bei **~15% Restbudget** bremsen (Puffer
   gegen Bursts — Anthropics Limiter reißt selbst hohe Limits bei Bursts
   kurz). Schwelle als benannte Konstante.
2. **Schlichte Warteanzeige**: „Token-Budget erschöpft — warte Xs bis zum
   Reset…", Request geht danach **automatisch** raus. Kein Abbrechen-/
   Trotzdem-Button (einfach, robust).

## 1. Header-Auswertung (die fehlende Grundlage)

Bei **jeder** Antwort (auch **erfolgreichen**, nicht nur 429) die Rate-Limit-
Header lesen und im Budget-Wächter (§2) ablegen:
- **Anthropic**: `anthropic-ratelimit-requests-remaining/-reset`,
  `-input-tokens-remaining/-reset`, `-output-tokens-remaining/-reset`,
  `-tokens-remaining/-reset`. Plus `retry-after` (schon gelesen, 0051).
- **Andere Provider (OpenAI/OpenRouter/…)**: haben **andere** Header
  (`x-ratelimit-*` o. Ä.) oder gar keine. Die Header-Extraktion **pro
  Provider-Typ** kapseln (der Provider weiß, welche Header er hat). Wo keine
  Header existieren (z. B. Ollama lokal), **graceful degradieren**: kein
  proaktives Budget, Fallback auf das bestehende reaktive Retry (0051) — die
  Drosselung darf einen header-losen Provider nicht blockieren.
- **Wichtig**: `map_http_status` (error.rs) wirft aktuell Status/Header/Body
  weg (jeder 429 → Unit-`RateLimited`). Die Header müssen **vor** diesem
  Verlust extrahiert werden.

## 2. Geteiltes Budget pro PROVIDER-IDENTITÄT (der Kern)

**Die zentrale Design-Entscheidung** (Stefan): Das Budget ist **pro Provider-
Identität**, nicht pro Aufruf-Zweck. Wenn derselbe Key als Haupt- UND
Zweitmeinungs-Provider dient, teilen sich beide **ein** Budget (Anthropic
limitiert **pro Organisation**, nicht pro logischem Provider in der App —
mehrere Aufruftypen mit demselben Key verbrauchen denselben Pool).

- Ein **Budget-Wächter** pro Provider-Identität. Identität = was das Limit
  teilt: **Key/Organisation + Modell-Klasse** (Anthropic trennt Limits pro
  Modell-Klasse). Kläre und beschreibe mir, woran du die Identität festmachst
  (Key-Hash? Base-URL+Modell? — es muss so grob sein, dass alle Aufrufe, die
  *tatsächlich* dasselbe Limit teilen, denselben Wächter treffen, aber nicht
  gröber).
- **Alle** Aufruftypen (Haupt-Chat, Zweitmeinung, Injection-Check, Summary,
  Auto-Titel, Notiz-Vorschlag, Notiz-Kürzung) gehen durch **denselben**
  Wächter, wenn sie dieselbe Identität nutzen.
- Der Wächter hält aus den Headern: Restbudget (RPM, Input-TPM, Output-TPM)
  + Reset-Zeitpunkte. Bei einem 429 aktualisiert er sich (Budget = 0 bis
  Reset).

## 3. Aufruf-Gate: vor dem Send prüfen, ggf. warten

Vor **jedem** `AiProvider::send()` (alle Typen): den Budget-Wächter fragen.
- Genug Budget (über der 15%-Schwelle für **alle** relevanten Zähler) → sofort
  senden.
- Unter der Schwelle bei irgendeinem Zähler → **warten** bis zum Reset-
  Zeitpunkt dieses Zählers (aus den Headern), dann senden. Während des Wartens
  das UI-Event (§4) feuern.
- **Schätzung des Request-Gewichts**: Bevor ein Request rausgeht, ist seine
  Input-Token-Zahl schätzbar (Post-Fencing, wie in 0057 §3.1 — **denselben
  Schätzer wiederverwenden**). Wenn der geschätzte Input das verbleibende
  Input-TPM-Budget überschreiten würde → warten. So wird das TPM-Limit *vorab*
  respektiert, nicht erst nach dem Überschreiten.
- Das Gate ersetzt **nicht** das bestehende Pacing (0051) und Retry — es
  ergänzt sie: proaktiv (Gate) + reaktiv (Retry als Sicherheitsnetz, falls die
  Schätzung/Header mal danebenliegen).

## 4. UI: Warteanzeige

Wenn das Gate wartet: ein **Chat-/Status-Event** an das Frontend, das anzeigt:
„Warte auf KI-Budget — nächster Versuch in Xs" (mit herunterzählender Zeit,
falls einfach). Verschwindet, sobald der Request rausgeht.
- **Deckt zusätzlich** den bestehenden Backlog-Punkt „kein UI-Feedback bei
  ~20min Provider-Stille" mit ab — dasselbe Event-Muster für „App wartet auf
  Provider".
- Kein Button (Entscheidung 2) — rein informativ, Request geht automatisch.

## Invarianten / Sicherheit
- **Redaction läuft weiter vor jedem Send** — das Gate sitzt *vor* dem Send,
  ändert nichts an der Redaction-Reihenfolge.
- **Ein header-loser Provider (Ollama) wird nie blockiert** — graceful
  degradieren auf reaktives Retry.
- Das Gate + Retry dürfen zusammen **kein** unbegrenztes Hängen erzeugen — das
  Warten hat eine sinnvolle Obergrenze (der Reset-Zeitpunkt ist endlich; falls
  ein Reset-Header fehlt/unsinnig ist, Fallback auf eine Max-Wartezeit, dann
  reaktives Retry). „Fehler containen" bleibt gewahrt.
- Das geteilte Budget verhindert die Doppel-Verbrauchs-Lücke (Haupt +
  Zweitmeinung mit demselben Key).

## Testbarkeit
- Header werden bei Erfolg UND 429 extrahiert (Mock mit gesetzten
  `anthropic-ratelimit-*`-Headern → Budget im Wächter korrekt).
- Zwei Aufruftypen mit **derselben** Identität → **ein** geteiltes Budget
  (nicht zwei); mit verschiedenen Identitäten → getrennt.
- Budget unter 15% → Gate wartet bis Reset, dann Send; UI-Event gefeuert.
- Geschätzter Input > Rest-Input-TPM → Gate wartet vorab.
- Header-loser Provider → nie blockiert, reaktives Retry greift.
- Kein unbegrenztes Hängen (Reset fehlt → Max-Wartezeit → Retry).
- Bestehende 0051-Retry- und Body-Timeout-Tests bleiben grün.

## Nicht Teil dieser Spec
- Der **adaptive Token-Bucket** (Rate über Zeit dynamisch anpassen, AWS-SDK-
  Muster) — das ist die *nächste* Stufe, falls das Header-basierte Gate nicht
  reicht. Erst das präzise Header-Gate, dann ggf. adaptiv.
- Die **einstellbaren Zweitmeinungs-Stufen** (weniger Requests) — eigener
  Backlog-Punkt; unabhängig, kann zusätzlich kommen.

## Abschluss
- `spec-reviewer` ERHÖHT (KI-Request-Pfad, Invarianten aus 0051/0057/
  Body-Timeout).
- CHANGELOG: „App liest jetzt die Rate-Limit-Header und drosselt proaktiv,
  statt blind ins Limit zu laufen; zeigt Wartezeiten an."
- Melde mir: woran du die **Provider-Identität** festmachst (§2 — der Kern),
  wie das Gate mit dem bestehenden Pacing/Retry zusammenspielt, wie
  header-lose Provider degradieren, und dass kein unbegrenztes Hängen möglich
  ist.
