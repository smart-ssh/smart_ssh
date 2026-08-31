# 0002-sudo-dual-text-matching

## Status
Accepted

## Kontext

`docs/specs/0002-filter-engine-spec.md` enthält an zwei Stellen Vorgaben zum
Umgang mit `sudo`/`doas`-Präfixen, die beim Implementieren in Spannung
zueinander stehen:

- Abschnitt 4.6 verlangt, das Präfix **vor dem Matching zu entfernen** und
  separat zu vermerken, "sodass später z. B. eine Regel 'alles mit sudo →
  immer Confirm' möglich ist" (Formulierung "später ... möglich" legt nahe:
  zum Zeitpunkt der Spec noch nicht konkret spezifiziert, wie).
- Der Testfall-Katalog (Abschnitt 6, Zeile 7) erwartet aber genau das
  Gegenteil als testbares Verhalten *jetzt schon*: `sudo apt update` soll
  `Confirm` ergeben, "falls Regel 'sudo → Confirm' aktiv" ist — eine Regel
  müsste also erkennen können, dass ein Kommando mit `sudo` aufgerufen wurde.

Die in Abschnitt 2 fest vorgegebenen Typen (`Rule`, `Pattern`) haben aber
kein eigenes Feld, um eine Regel an "elevated: true/false" zu binden —
`Pattern` kennt nur `Glob`/`Regex`/`Exact` gegen den Kommandotext. Diese ADR
wurde nicht explizit in Abschnitt 7 der Spec als offener Punkt gelistet,
ergab sich aber direkt und unausweichlich beim Implementieren von
`evaluate_rules` (`crates/core/src/filter/engine.rs`).

## Entscheidung

Beim Prüfen, ob eine Nutzerregel auf ein Teilkommando passt, wird das
`Pattern` der Regel gegen **zwei Text-Varianten** geprüft:

1. den Originaltext des Teilkommandos (inkl. `sudo`/`doas`-Präfix, falls
   vorhanden),
2. den um das Präfix bereinigten Text.

Matcht das Pattern gegen eine der beiden Varianten, gilt die Regel als
Treffer. Damit funktionieren beide in der Spec beschriebenen Fälle ohne
Erweiterung der Abschnitt-2-Typen:

- Eine Allow-Regel `"apt update"` matcht weiterhin sowohl `apt update` als
  auch `sudo apt update` (Abschnitt 4.6) — muss nicht doppelt gepflegt
  werden.
- Eine Regel `"sudo *"` (Confirm/Deny) kann gezielt auf jedes Kommando mit
  `sudo`-Präfix reagieren (Testfall 7).

## Konsequenzen

**Positiv:**
- Beide Spec-Anforderungen sind ohne Widerspruch und ohne Änderung der in
  Abschnitt 2 fest vorgegebenen Datenstrukturen erfüllt.
- Für Regel-Autoren intuitiv: ein Pattern wie `"sudo *"` verhält sich wie
  erwartet, ohne dass man wissen muss, dass intern normalisiert wird.

**Negativ / Trade-off:**
- Es ist mit den aktuellen Typen nicht ausdrückbar, eine Regel zu
  formulieren, die *nur* ohne `sudo` matchen soll (ein "aber nicht unter
  sudo"-Ausschluss). Ein Pattern wie `"apt update"` matcht implizit auch
  `sudo apt update`, selbst wenn das nicht gewollt wäre.
- Ein sehr generisches Pattern wie `Glob("*")` matcht trivial auf beiden
  Text-Varianten — das ändert am Ergebnis nichts (es hätte ohnehin
  gematcht), macht aber die doppelte Prüfung in diesem Fall überflüssig.
- Sollte künftig ein echtes Bedürfnis für elevated-spezifisches Matching
  entstehen (z. B. "matcht X, aber nur ohne sudo"), reicht Dual-Matching
  nicht mehr aus — dann braucht `Pattern`/`Rule` ein explizites
  `elevated: Option<bool>`-artiges Feld, und diese Entscheidung müsste
  revidiert werden.
