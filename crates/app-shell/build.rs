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
//! **`cargo:rerun-if-changed`** auf `.git/HEAD` sowie die Datei, auf die
//! `HEAD` zeigt (z. B. `.git/refs/heads/main`) — ohne das würde Cargo den
//! Build-Script-Output nach dem ersten Lauf cachen und ein inkrementeller
//! Build nach einem neuen Commit weiterhin den alten Hash zeigen. Beide
//! Pfade werden über `git rev-parse --git-dir`/`git symbolic-ref` ermittelt
//! statt fest als `.git/...` angenommen — ein Git-Worktree (`.git` ist dort
//! eine Datei mit einem Verweis, kein Verzeichnis) hätte den fest codierten
//! Pfad sonst falsch getroffen.
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
// `cargo test` das anzeigt (stiller toter Code). Die Erfolg/Leer/Fehler-
// Entscheidungslogik ist deshalb absichtlich trivial genug gehalten, um sie
// nicht separat testen zu müssen (`Result`/`Option`-Verkettung ohne eigene
// Fallunterscheidung); tatsächlich getestet ist stattdessen das *Format*,
// in dem der eingebettete Hash weiterverwendet wird
// (`crate::version::version_with_hash`, `src/version.rs`, läuft unter
// `cargo test -p app-shell`). Das reale build.rs-Verhalten selbst (Hash
// mit Git, `"unknown"` ohne) wurde manuell verifiziert — s. Commit-Text.

use std::process::Command;

fn main() {
    println!(
        "cargo:rustc-env=SMART_SSH_BUILD_HASH={}",
        resolve_commit_hash()
    );
    emit_rerun_triggers();
}

fn resolve_commit_hash() -> String {
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
/// Repo), wird still nichts registriert — derselbe "nie den Build
/// abbrechen"-Grundsatz wie bei [`resolve_commit_hash`]; ein dann fehlendes
/// Rerun-Trigger ist unschädlich, da `resolve_commit_hash` in diesem Fall
/// ohnehin nur `"unknown"` liefert.
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
    if let Some(head_ref) = run_git(&["symbolic-ref", "-q", "HEAD"]) {
        println!("cargo:rerun-if-changed={git_dir}/{head_ref}");
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
