# Architecture Decision Records (ADRs)

Ein ADR dokumentiert eine wichtige architektonische Entscheidung: welches
Problem gelöst wird, welche Optionen abgewogen wurden, welche Entscheidung
getroffen wurde und welche Konsequenzen sich daraus ergeben.

## Wann ein ADR anlegen?

- Bei Entscheidungen mit langfristiger Auswirkung (z. B. Wahl einer Library,
  Aufteilung `core`/`app-tauri`, Datenmodell, Sicherheitsarchitektur).
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

Nummern werden nie wiederverwendet oder umsortiert – auch verworfene
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
