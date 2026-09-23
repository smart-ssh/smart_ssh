# ADR 0060: Lizenzwechsel FSL-1.1-MIT → Apache License 2.0

Status: Angenommen
Bezug: docs/specs/0070-license-apache-2-0.md, Commit `ef756d3`

## Kontext

Das Repo stand bisher unter der Functional Source License 1.1 (FSL-1.1-MIT):
einer Non-Compete-Lizenz mit automatischer Umwandlung in eine MIT-Lizenz
nach zwei Jahren. Sie ist keine OSI-anerkannte Open-Source-Lizenz. Vor dem
1.0-Launch soll das Repo unter einer echten Open-Source-Lizenz stehen
(Release-Gate 1.0, G).

## Entscheidung

Apache License 2.0 (SPDX `Apache-2.0`) für das gesamte öffentliche Repo
`smart_ssh`. Gründe:

- OSI-anerkannt.
- Enthält eine ausdrückliche Patentlizenz.
- In Unternehmen ohne gesonderte Rechtsprüfung einsetzbar (Standardlizenz).
- Kompatibel mit der bestehenden `cargo deny`-Allow-Liste (Apache-2.0 steht
  dort bereits als zulässige Drittlizenz).

Keine externen Beitragenden vorhanden (alle Commits vom Rechteinhaber,
geprüft) — eine Zustimmung Dritter war daher nicht nötig.

Umsetzung: `LICENSE` (ohne Endung, unveränderter offizieller Text inkl.
unausgefüllter Appendix-Platzhalter) ersetzt `LICENSE.md`; eine minimale
`NOTICE` (Projektname + Copyright-Zeile) übernimmt den bisher im FSL-Text
stehenden Copyright-Vermerk; `Cargo.toml`, `deny.toml`-Kommentar,
`README.md` und das Frontend-Manifest (`package.json`/`package-lock.json`)
deklarieren durchgängig `Apache-2.0`.

### OP-1 — keine Dateikopfzeilen (Scope-Reduktion gegenüber dem Gate-Wortlaut)

Das Release-Gate nennt „Dateikopfzeilen" als Prüfpunkt. Im Repo gab es vor
diesem Wechsel keine einzige Datei mit Lizenz-, SPDX- oder Copyright-Kopf
(geprüft). Entscheidung (Stefan, 2026-09-22, Tor 1): keine Kopfzeilen
einführen. Apache 2.0 verlangt keine Dateiköpfe (der Appendix ist eine
Empfehlung, kein Pflichtteil der Lizenzerteilung); eine zentrale, eindeutige
`LICENSE`/`NOTICE` im Wurzelverzeichnis genügt rechtlich. Ein einzeiliger
SPDX-Kopf in jeder `.rs`/`.ts`/`.tsx`-Datei hätte praktisch jede Quelldatei
angefasst und wäre ein Merge-Konfliktrisiko mit jedem parallel laufenden
Worktree gewesen, ohne rechtlichen Mehrwert. Der Gate-Unterpunkt
„Dateikopfzeilen" gilt damit als geprüft (keine vorhanden) und bewusst
nicht eingeführt — das erfüllt den Wortlaut des Gates nicht buchstäblich,
ist aber eine bewusste, von Stefan getroffene Entscheidung. Bleibt jederzeit
nachrüstbar, ohne etwas zu brechen.

### OP-2 — minimale `NOTICE`

Entscheidung (Stefan, 2026-09-22, Tor 1): eine minimale `NOTICE` mit nur
Projektname und Copyright-Zeile, kein Sammelort für Drittlizenzen (das
bleibt ein eigenes Artefakt von BL-0054 — Apache-Konvention: jede Zeile in
`NOTICE` wird für Weiterverteiler zur Pflicht).

### OP-3 — rückwirkende Aussage im CHANGELOG

Entscheidung (Stefan, 2026-09-22, Tor 1): Der CHANGELOG-Eintrag erklärt
zusätzlich zum Wechsel ab jetzt, dass auch alle bereits veröffentlichten
Versionen unter Apache 2.0 genutzt werden dürfen — als alleiniger
Rechteinhaber möglich, aber unwiderruflich. Das README nennt frühere
Lizenzen bewusst nicht (beschreibt den Ist-Zustand; Historie steht im
CHANGELOG).

### Abweichung von der Changelog-Fragment-Konvention

Der übliche Arbeitsablauf verlangt für nutzerrelevante Änderungen ein
Fragment in `changelog.d/`, nicht eine direkte Änderung an `CHANGELOG.md`
(Grund: parallele Coder würden sonst denselben `[Unreleased]`-Abschnitt
ändern). Spec 0070 schreibt für diesen Wechsel ausdrücklich eine direkte
Änderung an `CHANGELOG.md` vor (Anforderung 3.8, Wortlaut §4.7, Teil des
einen atomaren Commits in §7.1) — bewusst, weil LICENSE-Text, `NOTICE` und
die CHANGELOG-Aussage zur Rückwirkung in einem einzigen, in sich
widerspruchsfreien Commit landen müssen; ein später beim Release
eingesammeltes Fragment hätte diese Atomarität aufgegeben. Dieser ADR hält
die Abweichung fest, damit sie nicht als Versehen erscheint.

## Konsequenzen

- Unumkehrbar für bereits veröffentlichten Code: einmal unter Apache 2.0
  veröffentlichter Code bleibt unter Apache 2.0 nutzbar.
- Auch alle früheren Versionen sind rückwirkend unter Apache 2.0 nutzbar
  (OP-3).
- ADRs und Specs, die FSL erwähnen (u. a. `docs/adr/0053-…`,
  `docs/adr/0054-…`, `docs/specs/0001-…`), bleiben als historische
  Dokumente unverändert.
- Keine Aussage zu Editionen oder Geschäftsmodell — dieser ADR betrifft
  ausschließlich die Lizenz des öffentlichen Repos `smart_ssh`.
