# ADR 0079 — Entscheidungen bei A4/A5/A6 (Spec 0085, Teil 2)

Status: angenommen · 2026-09-28 · Spec: `docs/specs/0085-elevated-channel-revocation-follow-ups.md`
Betrifft: `crates/app-logic/` (Doc-Kommentare), `crates/app-shell/src/commands/elevation.rs`,
`changelog.d/`

Gilt für **Teil 2** der Spec (A4, A5, A6). Teil 1 (A1–A3) ist in
`docs/adr/0078-elevated-channel-revocation-decisions.md` festgehalten.

## 1. Bewusst nicht behobener Fund: weitere falsche `app_shell::`-Querverweise außerhalb des A4-Scopes

A4/T12 zählt ausschließlich Kommentare, die noch als `crate::X` auf ein
`app-shell`-Element verweisen (Rest der Spec-0083/0084-Extraktion). Der
spec-reviewer fand in Runde 2 eine verwandte, aber andere Art Fehler: drei
Stellen, an denen ein Kommentar bereits `app_shell::…` schreibt, dabei aber
auf etwas zeigt, das seit der Spec-0084-Crate-Extraktion tatsächlich in
**app-logic** liegt (`app_shell::orchestration::run_one_round` in
`events.rs:678`, `app_shell::orchestration::log_command_execution(_failed)`
in `diagnostics.rs:45`, `app_shell::lib` — kein Modulpfad — in
`diagnostics.rs:62`).

Diese drei Stellen sind nicht Teil des A4-Auftrags: A4 zählt und behebt nur
die Richtung `crate::X → app_shell::X`, nicht bereits falsch in die andere
Richtung zeigende Verweise. Sie stammen aus der Spec-0084-Extraktion selbst
(Commit `ab993d5`), nicht aus dem A4-Sweep dieses Schritts, und wurden vom
A4-Sweep folgerichtig nicht berührt (T12 = 0 bleibt davon unberührt, weil
T12 sie nicht zählt).

Nicht behoben, weil außerhalb des Auftrags dieses Schritts (A4 nennt
ausdrücklich nur die `crate::…`-Form, T12 = 0 ist das einzige Kriterium).
Empfehlung an den Architekten: eigenes Backlog-Item für „Querverweise in
`app-logic`, die auf `app_shell::orchestration`/`app_shell::lib` zeigen,
korrigieren auf `crate::orchestration`/`app_shell`" — ggf. mit einer
robusteren Gate-Prüfung, die `app_shell::<modul>` gegen die tatsächlich in
`crates/app-shell/src` vorhandenen Module abgleicht, statt nur eine feste
Modulliste in eine Richtung zu zählen.

## 2. Der A4-Sweep selbst führte einen neuen Fehler ein (behoben)

Die mechanische Ersetzung `crate::local_server::LOCAL_SERVER_ID` →
`app_shell::local_server::LOCAL_SERVER_ID` (`crates/app-logic/src/dto.rs:98`)
war eine Verschlechterung: Die Konstante ist seit Spec 0084 §4 in derselben
Datei definiert (`dto.rs:39`), `app-shell` importiert sie nur. Der
Regex-basierte Sweep aus T12 kann das nicht unterscheiden — er ersetzt
mechanisch, ohne zu prüfen, ob das Ziel wirklich in `app-shell` liegt.
Behoben (spec-reviewer, Runde 1) über einen Intra-Doc-Link
(`[`LOCAL_SERVER_ID`]`) auf die tatsächliche, lokale Definition.
