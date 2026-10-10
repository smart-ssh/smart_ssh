# Spec 0061 — Vorausschauende, anbieterweite Rate-Limit-Drosselung

Status: umgesetzt
Zweck: Die App liest die Rate-Limit-Angaben des Anbieters und wartet vor
einer Anfrage, wenn das Budget knapp ist, statt blind in ein 429 zu laufen.
Bezüge: Spec 0051 (Wiederholung nach 429, Mindestabstand), Spec 0057
(Kontextaufbau, Token-Schätzung), Spec 0006.

## Ausgangslage

Die Kontext-Kompaktierung (Spec 0057) schützt vor dem Kontextfenster, nicht
vor dem Rate-Limit (Input-Tokens pro Minute, auf niedrigen Tarifen deutlich
kleiner). Mehrere kleine Anfragen pro Minute können das Budget sprengen,
ohne dass je eine Kompaktierung auslöst. Das ist eine andere Achse.

## Entscheidungen

1. **Konservativ drosseln:** schon bei etwa 15 % Restbudget bremst die App
   (Puffer gegen Bursts).
2. **Schlichte Warteanzeige:** Es erscheint „Token-Budget erschöpft — warte
   Xs bis zum Reset…"; die Anfrage geht danach automatisch raus. Es gibt
   keinen Abbrechen- oder „Trotzdem senden"-Knopf.

## 1. Auswertung der Header

Bei **jeder** Antwort des Anbieters (auch erfolgreichen, nicht nur 429)
werden dessen Rate-Limit-Header gelesen und gemerkt:

- **Anthropic:** Restbudget und Reset-Zeitpunkt für Anfragen, Input-Tokens,
  Output-Tokens und gesamt (`anthropic-ratelimit-*`), außerdem
  `retry-after` (Spec 0051).
- **Andere Anbieter** (OpenAI, OpenRouter …) haben andere Header
  (`x-ratelimit-*` o. ä.) oder keine. Wo keine Header existieren
  (z. B. Ollama lokal), gibt es kein vorausschauendes Budget: die
  Drosselung degradiert auf die reaktive Wiederholung aus Spec 0051 und
  blockiert einen header-losen Anbieter nie.
- Die Header werden vor einem eventuellen Fehler gelesen, sodass auch ein
  429 sie nicht verliert.

## 2. Geteiltes Budget je Anbieter-Identität

Das Budget gilt **je Anbieter-Identität**, nicht je Aufrufzweck. Anthropic
limitiert je Organisation (praktisch je Key) und je Modell-Klasse; dient
derselbe Key als Haupt- und Zweitmeinungs-Anbieter, teilen sich beide ein
Budget.

- Als Identität zählen Basis-URL, Modellname und ein Hash des API-Keys.
  Der exakte Modellname ist eine konservative Näherung für die
  Modell-Klasse: im schlimmsten Fall entstehen zwei getrennte Zähler für
  Modelle, die dasselbe Kontingent teilen, nie ein fälschlich geteilter.
- **Alle** Aufruftypen (Haupt-Chat, Zweitmeinung, Einschleusungs-Prüfung,
  Zusammenfassung, Auto-Titel, Notiz-Vorschlag, Notiz-Kürzung) nutzen
  denselben Zähler, sobald sie dieselbe Identität haben.
- Der Zähler hält Restbudget (Anfragen, Input-/Output-Tokens) und
  Reset-Zeitpunkte. Bei einem 429 steht das Budget bis zum Reset auf null.

## 3. Prüfung vor jedem Senden

Vor **jeder** Anfrage fragt die App den Zähler:

- Genug Budget (über der Schwelle für alle relevanten Zähler): sofort
  senden.
- Unter der Schwelle bei irgendeinem Zähler: warten bis zum Reset dieses
  Zählers, dann senden; währenddessen läuft die Warteanzeige (Abschnitt 4).
- **Gewicht der Anfrage:** Die Input-Tokens sind vorab schätzbar (derselbe
  Schätzer wie in Spec 0057). Würde die Schätzung das verbleibende
  Input-Budget überschreiten, wird vorab gewartet.
- Die Prüfung ergänzt Mindestabstand und Wiederholung (Spec 0051): proaktiv
  die Prüfung, reaktiv die Wiederholung als Sicherheitsnetz, falls
  Schätzung oder Header danebenliegen.

## 4. Warteanzeige

Wartet die App, zeigt die Oberfläche „Warte auf KI-Budget — nächster
Versuch in Xs", mit herunterzählender Zeit; die Anzeige verschwindet, sobald
die Anfrage rausgeht. Dasselbe Muster deckt allgemein „App wartet auf den
Anbieter" ab. Sie ist rein informativ.

## Sicherheits- und Konsistenzzusagen

- Redaction läuft weiterhin vor jedem Senden; die Prüfung sitzt davor und
  ändert daran nichts.
- Ein header-loser Anbieter wird nie blockiert.
- Kein unbegrenztes Hängen: Das Warten dauert höchstens bis zum Reset; fehlt
  der Reset-Zeitpunkt oder ist unsinnig, gilt eine Obergrenze von 90
  Sekunden, danach übernimmt die reaktive Wiederholung (Spec 0051).
- Das geteilte Budget verhindert den Doppelverbrauch, wenn Haupt- und
  Zweitmeinungs-Anbieter denselben Key nutzen.

## Grenzen

- Ein adaptiver Token-Bucket (Rate dynamisch über die Zeit anpassen) gibt
  es nicht; es gilt die headerbasierte Prüfung.
- Einstellbare Zweitmeinungs-Stufen (weniger Anfragen) sind unabhängig und
  nicht Teil dieser Spec.

## Akzeptanzfälle

- Header werden bei Erfolg und bei 429 gelesen und im Zähler abgelegt.
- Zwei Aufruftypen mit derselben Identität teilen ein Budget, mit
  verschiedenen sind sie getrennt.
- Budget unter der Schwelle: Warten bis Reset, dann Senden; die
  Warteanzeige erscheint.
- Geschätzter Input über dem Rest-Input-Budget: es wird vorab gewartet.
- Header-loser Anbieter: nie blockiert, die Wiederholung greift.
- Fehlender Reset: Maximalwartezeit, dann Wiederholung.
