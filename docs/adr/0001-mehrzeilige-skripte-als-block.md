# 0001-mehrzeilige-skripte-als-block

## Status
Accepted

## Kontext

`docs/specs/0002-filter-engine-spec.md`, Abschnitt 7, stellt die Frage offen,
wie die Filter-Engine mit mehrzeiligen Skripten umgehen soll — insbesondere
Here-Docs (`<<`, `<<-`) und Aufrufen wie `bash -c "..."` mit komplexem,
potenziell selbst mehrere Kommandos enthaltendem Inhalt.

Zwei Optionen standen laut Spec zur Wahl:

1. Versuchen, auch diese Fälle in Teilkommandos zu zerlegen und einzeln
   gegen die Präzedenz-Kette (Abschnitt 3) zu prüfen.
2. Sie komplett als einen einzigen, nicht weiter zerlegten Block behandeln
   und dafür immer `Confirm` erzwingen.

Die Spec selbst empfiehlt bereits Option 2 fürs MVP ("deutlich geringere
Fehleranfälligkeit als ein unvollständiger Parser"). Diese ADR macht diese
Empfehlung zu einer verbindlichen, dokumentierten Entscheidung, da sie beim
Implementieren des Parsers (`crates/core/src/filter/parser.rs`) tatsächlich
umgesetzt werden musste.

## Entscheidung

Ein Kommando wird als "mehrzeiliges Skript" erkannt und ohne weiteren
Zerlegungsversuch komplett als ein Block behandelt, wenn:

- es die Zeichenfolge `<<` enthält (Here-Doc/Here-String-Indikator), oder
- sein erstes Wort `bash`, `sh`, `zsh` oder `dash` ist (auch mit
  Pfad-Präfix, z. B. `/bin/bash`) und das zweite Wort `-c` ist.

In diesen Fällen liefert der Parser `ParseResult::Ambiguous` mit einer
entsprechenden Begründung, was in der Engine immer mindestens zu
`Decision::Confirm` führt — nie zu `AutoExec`, unabhängig davon, welche
Nutzerregeln konfiguriert sind. Es wird kein Versuch unternommen, den
Inhalt eines Here-Docs oder den String-Inhalt hinter `-c` in mehrere
Teilkommandos zu zerlegen (kein Chaining-Sub-Parsing).

> **Nachtrag (unabhängiger Review-Pass, Spec 0002):** Spec 0002, Abschnitt
> 4.6 wurde nachträglich um den Satz ergänzt: "Der Inhalt des
> `-c`-Arguments wird als eigenes Kommando … geprüft, nicht als
> undurchsichtiges Argument durchgewunken." Wörtlich gelesen widersprach
> das dieser ADR — ein Review-Fund zeigte den konkreten Schaden: eine
> Nutzer-`Deny`-Regel wie `Deny "docker *"` ließ sich über
> `bash -c "docker rm -f prod"` vollständig umgehen (landete bei
> `Confirm` statt `Deny`), was Spec 0002 Abschnitt 3's Garantie
> "Deny wird gar nicht erst zur Bestätigung angeboten" verletzt. Die
> Entscheidung dieser ADR bleibt in der Substanz unverändert (der
> `-c`-Inhalt wird weiterhin **nicht** in mehrere Teilkommandos zerlegt,
> `bash -c "cd /app && rm -rf /tmp/x"` bleibt ein einziger, nicht weiter
> aufgespaltener Block) — ergänzt wurde nur: das extrahierte `-c`/`-e`-
> Argument wird zusätzlich als EIN Kommando (via `resolve_effective_
> command`) gegen Hard-Blacklist und Nutzerregeln geprüft, mit demselben
> Rekursionsmechanismus wie Command-Substitution (`crates/core/src/filter/
> engine.rs`, `evaluate_parsed_explained`s `Ambiguous`-Zweig, s.
> `parser::extract_shell_c_style_code`). Das Ergebnis wird nur mit dem
> bisherigen `Confirm`-Baseline zum jeweils strengeren kombiniert — es
> kann also nur zu `Deny` **eskalieren**, nie zu `AutoExec` **herabgestuft**
> werden. Damit ist der wörtliche Spec-4.6-Satz erfüllt, ohne die
> Kern-Begründung dieser ADR (kein fehleranfälliges Mehrfach-Kommando-
> Parsing) aufzugeben.

## Konsequenzen

**Positiv:**
- Kein fehleranfälliger Parser für beliebig komplexe eingebettete Skripte
  nötig — deutlich kleinere Angriffsfläche für Parsing-Bugs, die
  versehentlich zu `AutoExec` führen könnten.
- Einfach nachvollziehbares, leicht zu auditierendes Verhalten.

**Negativ / Trade-off:**
- Auch harmlose, häufig genutzte Muster wie
  `bash -c "cd /app && ./deploy.sh"` landen immer bei `Confirm`, selbst wenn
  eine passende Allow-Regel für das eigentliche Kommando existieren würde.
  Das kann bei Workflows mit vielen `bash -c`-Aufrufen (z. B. CI/Deploy-
  Skripte) zu spürbar mehr Bestätigungs-Dialogen führen als nötig.
- Sollte sich das in der Praxis als zu störend erweisen, ist eine mögliche
  Weiterentwicklung, für eine eng begrenzte, explizit als sicher eingestufte
  Untermenge (z. B. `bash -c` mit ausschließlich literalen, unquotierten
  Kommandos ohne Variablen/Substitution) doch ein eingeschränktes Parsing zu
  erlauben — das müsste dann aber wieder eine eigene, sorgfältig geprüfte
  Entscheidung sein, keine stillschweigende Erweiterung dieser ADR.
