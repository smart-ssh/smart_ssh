# Specs

Eine Spec beschreibt, **wie sich das Produkt in einem Bereich verhält**: was
ein Nutzer sieht und tun kann, Regeln, Grenzfälle, Fehlerverhalten und
Sicherheitszusagen. Für Bereiche ohne Oberfläche (Build, CI, Release) ist
„der Nutzer" der Beitragende oder der, der ein Release-Paket bekommt.

Eine Spec beschreibt den Stand von `main`, keinen Plan. Sie ändert sich im
selben Pull Request wie der Code (siehe `CLAUDE.md`, Abschnitt „Spec
workflow").

## Form

```markdown
# Spec NNNN — Titel

Status: umgesetzt
Zweck: ein, zwei Sätze, worum es in diesem Bereich geht.
Bezüge: andere Specs und ADRs, auf die diese Spec aufbaut.

## 1. … (Verhalten, nach Themen gegliedert)

## n. Sicherheitszusagen      (falls der Bereich welche hat)

## n. Grenzen                  (was bewusst nicht gilt)
```

- **Verhalten statt Code.** Keine Dateipfade, Funktions-, Typ- oder
  Variablennamen, keine Zeilennummern. Was der Nutzer liest (Beschriftungen,
  Meldungen, Befehle, die ein Beitragender selbst eingibt), darf wörtlich
  stehen.
- **Keine Aufgabenlisten.** Kein „Ist-Stand vor dieser Spec", keine
  Commit-Reihenfolge, keine Nachweis- oder Testprotokolle, keine
  Backlog-Kennungen.
- **Begründungen gehören ins ADR** (`docs/adr/`). Die Spec nennt das ADR
  unter „Bezüge", wenn eine Entscheidung nicht offensichtlich ist.
- **Kennungen bleiben stabil.** Anforderungen tragen Kennungen wie `A2`
  oder `A4.3`, auf die Code, Tests und Kommentare verweisen. Beim
  Umschreiben bleibt eine Kennung erhalten, solange ihre Aussage bleibt;
  eine entfallene Kennung wird nicht neu vergeben.

## Aufgelöste Specs

Eine Spec, die nur eine Aufgabenliste war, ganz von einer anderen abgelöst
ist oder mit einer anderen zusammengelegt wurde, bleibt als kurzer Verweis
unter ihrer Nummer stehen, damit Verweise von außen nicht ins Leere gehen:

```markdown
# Spec NNNN — Titel (aufgelöst)

Status: aufgelöst
Nachfolger: Spec XXXX (Abschnitt …), ADR YYYY

Ein, zwei Sätze, wohin der Inhalt gewandert ist.
```

Nummern werden nicht neu vergeben. Eine neue Spec bekommt die höchste
vorhandene Nummer + 1.
