# Spec 0051 — Rate-Limit-Behandlung für KI-Anfragen (429)

Status: umgesetzt
Zweck: Ein Rate-Limit des KI-Anbieters (HTTP 429) ist ein temporärer,
erwartbarer Zustand. Die App wartet und versucht es erneut, statt
stehenzubleiben, und meldet bei endgültigem Scheitern verständlich.
Bezüge: Spec 0006 (Anbieter), Spec 0024/0047 (Fehlercodes, Sprache), Spec
0026/0039 (Zweitmeinung, Fencing), Spec 0061 (vorausschauende Drosselung).

## Teil 1 — Wiederholung mit Wartezeit

Bei HTTP 429 wiederholt die App die Anfrage automatisch:

- Ein `Retry-After`-Header (Sekunden) wird gelesen und respektiert; er
  hat Vorrang.
- Ohne ihn gilt exponentielles Backoff mit Jitter (beginnend bei etwa
  einer halben Sekunde, je Schritt verdoppelt).
- Es gibt eine **harte Obergrenze**: höchstens 4 Versuche (einer plus bis
  zu drei Wiederholungen) und eine Gesamtwartezeit von 20 Sekunden über
  alle Versuche. Danach gilt Teil 3.
- Die Regel gilt für **alle** KI-Anfragen, die ein 429 bekommen können —
  Haupt-Chat, Risiko-Zweitmeinung, Einschleusungs-Prüfung, Auto-Titel,
  Notiz-Vorschlag. Die Nebenpfade sind Best-Effort; sie dürfen den Chat
  nicht minutenlang stillstehen lassen, daher die kurze Gesamtdeckelung.
- Eine Wiederholung sendet exakt dieselbe, bereits redigierte Anfrage;
  Redaction läuft vor jedem (Wieder-)Versand, kein Retry-Pfad umgeht sie.

## Teil 2 — Anfragen nicht als Burst

Alle KI-Anfragen einer Sitzung laufen strikt seriell, und zwischen zwei
Anfragen derselben Sitzung liegt ein kleiner Mindestabstand (300 ms), damit
zwei intern ausgelöste Anfragen nicht im selben Rate-Limit-Fenster landen.
Der Abstand ist für Nutzer nicht spürbar. Er ersetzt weder die
Wiederholung (Teil 1) noch die vorausschauende Drosselung (Spec 0061).

## Teil 3 — Verständliche Fehlermeldung

Scheitert eine Anfrage auch nach den Wiederholungen, erscheint eine
eigene Meldung: „Der KI-Anbieter drosselt gerade die Anfragen (Rate
Limit). Bitte kurz warten und erneut senden." (DE und EN). Sie ist als
eigener Fall erkennbar, getrennt von der allgemeinen „Provider-Konfiguration
prüfen"-Meldung, und weist aufs Warten hin, nicht auf die Konfiguration.

## Sicherheits- und Konsistenzzusagen

- Redaction läuft vor jedem (Wieder-)Versand.
- Kein Secret und kein Key im Log eines 429.
- Die Wiederholungsschleife hat eine harte Obergrenze (Versuche **und**
  Gesamtzeit); es gibt weder Endlosschleife noch stilles Hängen.

## Akzeptanzfälle

- 429 mit `Retry-After`, der zweite Versuch gelingt: Die App wartet die
  angegebene Zeit und die Runde läuft normal weiter.
- Dauerhaftes 429: Nach der Obergrenze erscheint die Rate-Limit-Meldung,
  die Sitzung hängt nicht.
- Mehrere KI-Anfragen je Nachricht werden nicht quasi-gleichzeitig
  abgefeuert.
