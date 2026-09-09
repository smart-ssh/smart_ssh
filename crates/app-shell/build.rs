//! Bettet den kurzen Git-Commit-Hash des gebauten Stands als Compile-Zeit-
//! Konstante ein (Spec 0052, Abschnitt 2) — `crate::version::
//! BUILD_COMMIT_HASH` liest ihn über `env!("SMART_SSH_BUILD_HASH")`. Die
//! Version selbst wird **nicht** hier eingebettet, sondern bleibt bei ihrer
//! bestehenden Quelle (`tauri.conf.json` über `context.package_info()`,
//! Spec 0048) — nur der Hash kommt neu dazu.
//!
//! **Fallback ohne Git** (z. B. ein Build aus einem Tarball ohne
//! `.git`-Verzeichnis): `"unknown"` statt eines Build-Abbruchs — ein
//! fehlender Hash ist rein kosmetisch (Log/Über-Dialog/Titelzeile zeigen
//! `"unknown"` statt eines Hashes), niemals ein Grund, den ganzen Build
//! scheitern zu lassen.
//!
//! **Override-Hook** (additiv, rein generisch): ein konsumierender Build
//! (ein anderer Workspace, der diese Crate als Pfad-/Submodule-Dependency
//! nutzt) kann den einzubettenden Hash über die Umgebungsvariable
//! `SMART_SSH_BUILD_HASH_OVERRIDE` setzen — Cargo isoliert `cargo:rustc-
//! env` sonst strikt auf diese Crate, ein solcher Build könnte den `git
//! rev-parse`-Pfad unten also nicht anders beeinflussen und würde immer
//! den Hash *dieses* Repo-Checkouts einbetten, nie seinen eigenen. Ohne
//! die Variable (der Fall für dieses Repo selbst) verhält sich alles
//! exakt wie zuvor.
//!
//! **`cargo:rerun-if-changed`** auf `.git/HEAD` sowie die Datei, auf die
//! `HEAD` zeigt (z. B. `.git/refs/heads/main`) — ohne das würde Cargo den
//! Build-Script-Output nach dem ersten Lauf cachen und ein inkrementeller
//! Build nach einem neuen Commit weiterhin den alten Hash zeigen. Beide
//! Pfade werden über `git rev-parse --git-dir`/`--git-common-dir`/`git
//! symbolic-ref` ermittelt statt fest als `.git/...` angenommen — u. a.
//! wegen eines `git worktree`-Checkouts (dieses Repo hat davon welche unter
//! `.claude/worktrees/`): `HEAD` liegt dort worktree-lokal, `refs/heads/…`
//! aber im gemeinsamen Git-Verzeichnis — s. `emit_rerun_triggers`-Doc-
//! Kommentar für die genaue Unterscheidung.
//!
//! Spielt mit jedem lokalen Release-Build (`cargo tauri build`, ausgeführt
//! aus einem normalen Git-Checkout dieses Repos — es gibt aktuell **kein**
//! dediziertes macOS-Release-Skript in diesem öffentlichen Repo, nur
//! `scripts/tauri-dev.sh`/`scripts/setup-macos-dev-signing.sh` für den
//! Dev-Modus) und mit `.github/workflows/release.yml` (`actions/
//! checkout@v4`, auch als Shallow-Clone: `HEAD` und der aktuelle
//! Commit-Objekt sind auch bei `fetch-depth: 1` vorhanden, `git rev-parse
//! --short HEAD` funktioniert also) zusammen, ohne dass dort etwas
//! geändert werden muss: beide bauen aus einem echten Git-Checkout (kein
//! Tarball-Export), `git` ist in beiden Umgebungen vorhanden — der
//! Erfolgspfad greift, kein `"unknown"`-Fallback nötig. Ein privates
//! `official.yml` (falls es eines gibt) ist von hier aus nicht einsehbar,
//! dürfte aber demselben Muster folgen (echter Checkout statt Tarball).

// Spec-Testbarkeit (Abschnitt 6, "Ein Build mit Git liefert einen Hash; ein
// simuliertes Build ohne Git-Zugriff liefert `"unknown"`"): `cargo test`
// führt Build-Scripts nie als eigenes Test-Target aus (nur `lib`/`bin`/
// `tests`/`bench`-Targets, s. https://doc.rust-lang.org/cargo/reference/cargo-targets.html)
// — ein `#[cfg(test)] mod tests` hier würde also nie laufen, ohne dass
// `cargo test` das anzeigt (stiller toter Code). Eine Auslagerung der
// Entscheidungslogik in ein separat testbares, von `build.rs` und `src/`
// gemeinsam genutztes Modul wäre für den Umfang hier (eine Env-Var-Prüfung
// plus die bestehende `Result`/`Option`-Verkettung) unverhältnismäßig viel
// zusätzliche Struktur; tatsächlich getestet ist stattdessen das *Format*,
// in dem der eingebettete Hash weiterverwendet wird
// (`crate::version::version_with_hash`, `src/version.rs`, läuft unter
// `cargo test -p app-shell`). Das reale build.rs-Verhalten selbst (Hash
// mit Git, `"unknown"` ohne, Override gesetzt/leer/mehrzeilig) wurde
// manuell verifiziert — s. Commit-Text.

use std::process::Command;

fn main() {
    println!(
        "cargo:rustc-env=SMART_SSH_BUILD_HASH={}",
        resolve_commit_hash()
    );
    println!("cargo:rerun-if-env-changed=SMART_SSH_BUILD_HASH_OVERRIDE");
    emit_rerun_triggers();
}

/// Additiver Override-Hook: Cargo isoliert `cargo:rustc-env` strikt auf die
/// eigene Crate, ein konsumierender Build (ein anderer Workspace, der
/// `app-shell` als Pfad-/Submodule-Dependency nutzt) kann diesem
/// Build-Script sonst keinen anderen Hash unterschieben und bettet immer
/// den Hash *dieses* Repos ein statt seines eigenen.
/// `SMART_SSH_BUILD_HASH_OVERRIDE` (gesetzt, nicht-leer nach dem Trimmen)
/// hat Vorrang vor dem git-Pfad; ohne die Variable (der Fall für die
/// Community Edition, die sie nie setzt) bleibt das Verhalten exakt wie
/// zuvor, inklusive des `"unknown"`-Fallbacks ohne Git.
///
/// Spec-Reviewer-Fund: ein getrimmter, aber intern mehrzeiliger Override-
/// Wert würde unverändert in `println!("cargo:rustc-env=…={value}")`
/// landen — Cargo interpretiert *jede* mit `cargo:` beginnende Zeile eines
/// Build-Scripts als eigene Direktive, ein eingebetteter Zeilenumbruch
/// könnte also zusätzliche, nicht vorgesehene Direktiven einschleusen.
/// Kein Privilegiengewinn (wer die Variable setzt, kontrolliert ohnehin
/// den gesamten Build), aber ein plausibler Fall für eine versehentlich
/// falsch befüllte CI-Variable (z. B. aus einem mehrzeiligen
/// `git log`-Format) — deshalb bewusst verworfen statt stillschweigend
/// durchgereicht: fällt dann auf den git-Pfad zurück, mit einer
/// `cargo:warning`-Zeile als Hinweis. Eine defensive Längenkappung
/// verhindert zusätzlich einen unangemessen langen Wert (kosmetisch
/// relevant: landet in Log/Über-Dialog/Titelzeile, Spec 0052).
fn resolve_commit_hash() -> String {
    if let Ok(override_hash) = std::env::var("SMART_SSH_BUILD_HASH_OVERRIDE") {
        let trimmed = override_hash.trim();
        if !trimmed.is_empty() {
            if trimmed.chars().any(|c| c.is_control()) || trimmed.len() > 128 {
                println!(
                    "cargo:warning=SMART_SSH_BUILD_HASH_OVERRIDE ignoriert (enthält \
                     Steuerzeichen oder ist länger als 128 Zeichen) — falle auf \
                     `git rev-parse` zurück"
                );
            } else {
                return trimmed.to_string();
            }
        }
    }

    Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Ermittelt `.git`-Verzeichnis und den aktuellen Branch-Ref über `git`
/// selbst (statt `.git/HEAD` hart anzunehmen) und registriert beide als
/// `rerun-if-changed`-Trigger. Schlägt die Ermittlung fehl (kein Git, kein
/// Repo), wird still nichts registriert.
///
/// Spec-Reviewer-Fund: der ursprüngliche Kommentar hier berief sich darauf,
/// dass Cargo ein Build-Script ohne jede registrierte `rerun-if-*`-
/// Direktive bei jeder Änderung im Paket neu ausführt ("nie stale" auch
/// ganz ohne Git). Das gilt nicht mehr uneingeschränkt: `main()` gibt
/// inzwischen immer `cargo:rerun-if-env-changed=SMART_SSH_BUILD_HASH_
/// OVERRIDE` aus, *bevor* diese Funktion aufgerufen wird — sobald
/// irgendeine `rerun-if-*`-Direktive vorliegt, entfällt laut Cargo-
/// Dokumentation der Nur-ohne-jede-Direktive-Fallback. Praktisch folgenlos
/// bleibt das trotzdem: der einzige Fehlerfall hier ist "kein Git
/// verfügbar", und dann liefert [`resolve_commit_hash`] ohnehin dauerhaft
/// `"unknown"` — es gibt keinen aktuelleren Wert, der stale werden könnte.
///
/// Spec-Reviewer-Fund (Spec 0052, Review dieses Schritts): `HEAD` selbst
/// liegt worktree-lokal (`git rev-parse --git-dir`), aber `refs/heads/…`
/// liegt bei einem `git worktree`-Checkout im **gemeinsamen** Git-
/// Verzeichnis aller Worktrees, nicht im worktree-lokalen — `--git-dir`
/// dafür zu verwenden hätte in einem Worktree auf einen nicht
/// existierenden Pfad gezeigt (harmlos dank des Verhaltens oben, aber
/// ohne den beabsichtigten präzisen Trigger). `--git-common-dir` ist in
/// einem normalen (Nicht-Worktree-)Checkout identisch zu `--git-dir`,
/// unterscheidet sich also nur dort, wo es tatsächlich nötig ist.
fn emit_rerun_triggers() {
    let Some(git_dir) = run_git(&["rev-parse", "--git-dir"]) else {
        return;
    };
    println!("cargo:rerun-if-changed={git_dir}/HEAD");

    // `HEAD` selbst zeigt bei einem normalen Branch-Checkout nur auf einen
    // Ref (`ref: refs/heads/main`) — die eigentliche Commit-ID steht in der
    // Ref-Datei, die dieser Verweis referenziert. Nur die zusätzlich zu
    // beobachten (statt `.git/HEAD` allein) schließt genau den Fall, den
    // die Spec ausdrücklich nennt: ein neuer Commit auf dem aktuellen
    // Branch ändert `.git/HEAD` selbst nicht, nur die Ref-Datei dahinter.
    let common_dir = run_git(&["rev-parse", "--git-common-dir"]).unwrap_or(git_dir);
    if let Some(head_ref) = run_git(&["symbolic-ref", "-q", "HEAD"]) {
        println!("cargo:rerun-if-changed={common_dir}/{head_ref}");
    }
}

fn run_git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let trimmed = String::from_utf8(output.stdout).ok()?.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}
