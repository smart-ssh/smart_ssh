# Spec: Glob `*` überquert `/` nicht mehr (pfadförmige Muster)

Status: Entwurf
Repo: **öffentlich** `smart_ssh`, `crates/core` (Filter-Engine/Glob-Matching)
Modul: Das Glob-Matching der Filter-Engine (Regel-Auswertung)
Abhängigkeiten: Filter-Engine + Präzedenz (0002), Regel-Test-Panel (0009),
großer Spec-Audit (0009/0002, woher der Fund stammt)
Release-Gate: **C (MUSS)** — sicherheitskritisch, adversarial zu prüfen.

> **Das Problem (Filter-Engine-Umgehung):** Eine Allow-Regel wie
> `cat /var/log/*` matcht aktuell auch `cat /var/log/../../../etc/shadow`,
> weil der Glob-`*` über `/`-Grenzen hinweggeht. Ein Angreifer (oder ein
> KI-Vorschlag) könnte eine harmlos aussehende, legitim erteilte Allow-Regel
> ausnutzen, um Zugriff auf ganz andere Pfade zu bekommen — die Filter-Engine
> „erlaubt" dann etwas, das der Nutzer nie freigeben wollte.
> **Priorität ERHÖHT + adversariale Prüfung** (Filter-Engine-Invariante:
> „kann ein Kommando die Prüfung umgehen").

## Die Design-Entscheidung (aus dem Backlog)

Ein pauschales `literal_separator` für **alle** Globs würde den Normalfall
brechen: Kommando-Argument-Globs, die legitim Slashes enthalten (z. B. ein
Glob über URL-artige Argumente), würden nicht mehr matchen. Deshalb:

**Ein separater Glob-Modus für pfadförmige Muster.** Ein Muster, das wie ein
**Pfad** aussieht (beginnt mit `/`, oder ist klar ein Dateipfad-Argument),
wird mit `literal_separator = true` gematcht (`*` überquert `/` NICHT). Andere
Globs (Nicht-Pfad-Argumente) behalten das bisherige Verhalten (`*` überquert
`/`, wie bisher).

**Die Abgrenzung „was ist ein pfadförmiges Muster" ist der Kern der Spec** —
sie muss präzise und adversarial-fest sein (siehe unten).

## 1. Erkennung pfadförmiger Muster

Kläre und definiere präzise: **Wann gilt ein Glob-Muster als „pfadförmig"**
(→ `literal_separator`)?
- Der klare Fall: Muster **beginnt mit `/`** (absoluter Pfad, wie
  `/var/log/*`).
- Zu klären: relative Pfade (`./foo/*`, `foo/bar/*`)? Muster mit `/` irgendwo
  drin? **Beschreibe mir deinen Erkennungs-Ansatz** und die Grenzfälle, bevor
  du ihn festzurrst — das ist die eine echte Design-Entscheidung.
- **Konservativ im Zweifel**: Wenn unklar, ob pfadförmig, lieber
  `literal_separator` anwenden (strenger) als nicht — eine zu strenge Regel
  matcht im Zweifel *weniger* (Nutzer muss dann bestätigen, statt dass etwas
  fälschlich auto-erlaubt wird). Sicherheit vor Bequemlichkeit.

## 2. Das eigentliche Matching

- Pfadförmiges Muster → Glob mit `literal_separator = true`: `*` matcht
  **kein** `/`. `/var/log/*` matcht `/var/log/foo`, aber **nicht**
  `/var/log/sub/foo` und **nicht** `/var/log/../etc/shadow`.
- Nicht-pfadförmig → bisheriges Verhalten unverändert.
- **Path-Traversal**: `..` in einem gematchten Pfad darf nicht dazu führen,
  dass das Muster auf etwas außerhalb matcht. Der akute SFTP-Traversal-Fall
  ist bereits separat über einen lexikalischen Pfad-Normalizer entschärft —
  **prüfe, ob dieser Normalizer auch hier greift** (idealerweise: Pfad erst
  normalisieren, dann gegen das Muster matchen, sodass `../` gar nicht erst
  ins Matching kommt). Kläre das Zusammenspiel und beschreibe es mir.

## 3. Bestehende Regeln / Migration

- Bestehende `User`-Regeln in der DB ändern sich nicht — nur ihre
  **Auswertung** wird strenger. Eine bestehende `Allow: cat /var/log/*`-Regel
  matcht ab dem Fix keine `../`-Ausbrüche mehr. Das ist die **gewollte**
  Verschärfung (kein Datenverlust, keine Migration nötig).
- Prüfe, ob dadurch ein **legitimer** bestehender Anwendungsfall bricht (ein
  Nutzer, der bewusst `*` über `/` in einem Pfad-Glob nutzt) — unwahrscheinlich
  bei pfadförmigen Mustern, aber im CHANGELOG als Verhaltensänderung nennen.

## 4. Adversariale Prüfung (ERHÖHT — Pflicht)

Erfinde und prüfe konkrete Umgehungsversuche gegen die neue Logik:
- `/var/log/../../etc/shadow`, `/var/log/../log/../../etc/passwd`
- Ungewöhnliches Quoting, Whitespace-Tricks, Groß-/Kleinschreibung im Pfad
- Symlink-artige Konstrukte, doppelte Slashes (`/var//log/*`), `.`-Segmente
  (`/var/log/./x`)
- Ein Muster, das *fast* pfadförmig aussieht, aber die Erkennung austricksen
  könnte
- Relative Ausbrüche, falls relative Pfade als pfadförmig gelten
Dokumentiere die Testfälle und dass sie **nicht** durchkommen.

## Invarianten / Sicherheit
- Eine Allow-Regel mit pfadförmigem Muster kann **nie** auf einen Pfad
  außerhalb ihres Verzeichnisses matchen (kein `../`-Ausbruch, kein
  `/`-Übersprung).
- Im Zweifel strenger (weniger auto-erlaubt) — Sicherheit vor Bequemlichkeit.
- Nicht-pfadförmige Globs unverändert (kein Bruch des Normalfalls).
- Zusammenspiel mit dem bestehenden Pfad-Normalizer geklärt (keine doppelte/
  widersprüchliche Behandlung).

## Testbarkeit
- `Allow: cat /var/log/*` matcht `/var/log/syslog` ✅, matcht NICHT
  `/var/log/../../etc/shadow` ✅, matcht NICHT `/var/log/sub/deep` ✅.
- Nicht-pfadförmiges Glob (Kommando-Argument ohne führenden `/`) verhält sich
  wie bisher (Regressionstest, dass der Normalfall nicht bricht).
- Die adversarialen Fälle (§4) alle als „matcht nicht / führt zu Confirm".
- Regel-Test-Panel (0009): zeigt das strengere Verhalten korrekt (ein
  `../`-Kommando, das früher „Allow" zeigte, zeigt jetzt „Confirm/kein Match").
- Regressionstest gegen den ungefixten Stand (der `../`-Ausbruch matcht
  vorher, nachher nicht).

## Nicht Teil dieser Spec
- Der bereits entschärfte akute SFTP-Traversal (lexikalischer Normalizer) —
  nur das Zusammenspiel prüfen.
- Andere Glob-Semantik-Fragen außerhalb der `*`-über-`/`-Frage.

## Abschluss
- `spec-reviewer` **ERHÖHT mit adversarialer Haltung** (das ist genau ein
  Fall, wo 5–10 erfundene Umgehungsversuche gedanklich gegen den Code laufen
  müssen).
- CHANGELOG (Security-Kategorie): „Glob-`*` in pfadförmigen Allow-Regeln
  überquert keine Verzeichnisgrenzen mehr".
- Melde mir: deinen Erkennungs-Ansatz für „pfadförmig" (§1), das Zusammenspiel
  mit dem Normalizer (§2), die adversarialen Testfälle (§4), und ob ein
  bestehender legitimer Fall bricht (§3).
