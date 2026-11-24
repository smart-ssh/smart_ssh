# Architecture Decision Records (ADRs)

Ein ADR dokumentiert eine wichtige architektonische Entscheidung: welches
Problem gelöst wird, welche Optionen abgewogen wurden, welche Entscheidung
getroffen wurde und welche Konsequenzen sich daraus ergeben.

## Wann ein ADR anlegen?

- Bei Entscheidungen mit langfristiger Auswirkung (z. B. Wahl einer Library,
  Aufteilung `core`/`app-shell`, Datenmodell, Sicherheitsarchitektur).
- Wenn mehrere sinnvolle Alternativen existierten und eine Begründung für
  spätere Leser:innen (inkl. zukünftiges Ich) sinnvoll ist.
- Nicht für triviale, leicht rückgängig zu machende Entscheidungen.

## Nummerierung & Dateiformat

ADRs werden fortlaufend nummeriert, beginnend bei `0001`, vierstellig,
gefolgt von einem kurzen, sprechenden Titel in Kebab-Case:

```
0001-titel.md
0002-ein-anderer-titel.md
0003-noch-ein-titel.md
```

Die nächste freie Nummer zeigt `ls docs/adr | tail` (vor dem letzten Push
auch gegen Basis-Branch und offene PRs prüfen). Nummern werden nie
wiederverwendet oder umsortiert – auch verworfene
Entscheidungen behalten ihre Nummer und werden stattdessen als
"Superseded" bzw. "Deprecated" markiert (siehe unten).

## Empfohlene Struktur eines ADR

```markdown
# 0001-titel

## Status
Proposed | Accepted | Superseded by 0004 | Deprecated

## Kontext
Welches Problem/welche Fragestellung liegt vor?

## Entscheidung
Was wurde entschieden?

## Konsequenzen
Was folgt daraus (positiv wie negativ)?
```

## Status-Vokabular, Index und Prüfung

Der Abschnitt `## Status` beginnt in seiner ersten Zeile mit genau einem von:
`Proposed`, `Accepted`, `Superseded by NNNN` (die Nummer muss existieren) oder
`Deprecated`. Zusätze (Spec, Issue, Datum, Erklärung) folgen nach ` · ` in
derselben Zeile oder in den Zeilen danach.

`INDEX.md` ist die generierte Übersicht aller ADRs (Nummer, Titel, Status,
Datei) und wird nicht von Hand bearbeitet. `npm run check-adr` (Teil von
`npm run lint`, Skript `scripts/check-adr.mjs`) schlägt fehl bei doppelter
Nummer, falschem Dateinamen, fehlendem oder unbekanntem Status, unbekanntem
Nachfolger und veraltetem Index. Den Index erzeugt
`node scripts/check-adr.mjs --write-index`; mit `--dir <pfad>` und
`--index <pfad>` läuft die Prüfung auf einem anderen ADR-Ordner.
