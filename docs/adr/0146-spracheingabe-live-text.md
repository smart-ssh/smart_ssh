# ADR 0146 — Spracheingabe: Live-Text während der Aufnahme

## Status

Accepted · Issue: #595 · Spec 0108 · ändert ADR 0135, Entscheidungen 3 und 4

## Kontext

Der erkannte Text soll schon während der Aufnahme im Eingabefeld erscheinen. ADR 0135 liefert den Text genau einmal als Ergebnis von `stop` (Entscheidung 3) und lädt das Modell je Erkennung (Entscheidung 4).

## Entscheidungen

1. **Vorläufiger Text in einem eigenen Ereignis.** `speech-partial-text { text, revision }` trägt den gesamten vorläufigen Text bisher. Er bleibt auf dem Rechner und löst nie ein Senden aus. Der endgültige Text bleibt der Rückgabewert von `stop` und wird damit weiter genau einmal ausgeliefert; Entscheidung 3 gilt dafür unverändert, nur der Zusatz „nie in einem Ereignis“ entfällt für den vorläufigen Text. Die Revision wächst über Aufnahmen hinweg; die Oberfläche ignoriert Ereignisse mit kleinerer oder gleicher Revision und alle außerhalb des Zustands „Aufnahme“. Die Aufnahme meldet nur, solange sie noch läuft.
2. **Modell je Aufnahme statt je Erkennung.** Die Engine kann eine Sitzung öffnen (`Transcriber::open_session`), die das Modell hält; Live-Durchläufe und der abschließende Durchlauf teilen sie, danach wird sie freigegeben. Engines ohne Sitzung (Standard) laden weiter je Aufruf. Das ersetzt Entscheidung 4 von ADR 0135.
3. **Gleitendes Fenster.** Alle ~1 s läuft höchstens ein Durchlauf (der nächste beginnt erst nach dem Ende des vorigen). Er erkennt nur die letzten ~25 s; älterer Ton wird in Stücken von ~20 s einmal erkannt und bleibt als fester Text. Schnitte mitten im Wort sind möglich; der abschließende Durchlauf über die ganze Aufnahme ist maßgeblich. So wächst der Aufwand nicht mit der Aufnahmelänge.
4. **Ein Fehler eines Live-Durchlaufs** lässt den vorläufigen Text unverändert; nur der abschließende Durchlauf meldet `speech_transcription_failed`.
