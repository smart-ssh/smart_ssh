//! Dateizugriff für den `ssh_config`-Import (Spec 0075, §4.1 und §7.2).
//!
//! **Dies ist der einzige Teil, der das Dateisystem anfasst** — und damit
//! genau der Teil, der die Grenzen aus §3.3, die `Include`-Tiefe und die
//! Schleifenerkennung durchsetzt (§3.1.4, §5.6). `core` bekommt von hier
//! nur fertige Bytes und weiß nicht, wo sie herkamen.
//!
//! Dass die Rekursion **allein hier** liegt, ist die Eigenschaft, die §5.6
//! trägt: Eine sich selbst einbindende Datei ist eine Meldung, kein
//! Hänger und kein Speicherdruck. Die Messung aus §9/M-1 hat gezeigt, wie
//! das andernfalls ausgeht — dort beendete derselbe Fall den Prozess mit
//! einem Stapelüberlauf, den kein Test abfangen kann.

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use ssh_manager_core::profiles::ssh_config::{
    parse_source, FileParse, ImportSource, SkipReason, SkippedKind, SkippedReport,
};

/// §3.3 — die Grenzen gelten **über alle gelesenen Dateien zusammen**, nicht
/// je Datei. Eine Grenze je Datei würde durch eine Kette von
/// `Include`-Dateien umgangen (§9, Anmerkung zur Grenze).
pub const MAX_TOTAL_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_TOTAL_LINES: u64 = 100_000;
pub const MAX_HOST_BLOCKS: usize = 2_000;
/// §3.1.4: Die gewählte Datei ist Tiefe 0. Ein `Include` **in** einer Datei
/// der Tiefe 3 wird nicht mehr gefolgt.
pub const MAX_INCLUDE_DEPTH: u8 = 3;

/// Was mit einer Datei geschah. Die Vorschau nennt jede davon mit vollem
/// Pfad, in Lesereihenfolge (§3.1.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileReport {
    pub path: String,
    pub depth: u8,
    pub status: FileStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    Read,
    /// §3.1.4a — übersprungen, **ohne** Inhalt zu melden.
    NotSshConfig,
    /// §3.1.4: lässt den Import nicht scheitern.
    Unreadable,
    /// §3.1.4: je Import höchstens einmal gelesen.
    AlreadyRead,
}

/// Gründe, aus denen der **ganze** Import scheitert und nichts anlegt
/// (§3.1.13, §3.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportAbort {
    ChosenUnreadable,
    /// §3.1.4a, letzter Satz: Für die **gewählte** Datei gilt §3.1.13.
    ChosenNotSshConfig,
    TotalBytes,
    TotalLines,
    HostBlocks,
    /// Trägt Datei und Zeile, **nicht** den Wert.
    ValueTooLong {
        file: String,
        line: u32,
    },
    /// In keiner gelesenen Datei ein `Host`-Block (§3.1.13).
    NoHostBlock,
}

impl std::fmt::Display for ImportAbort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportAbort::ChosenUnreadable => {
                write!(f, "Die gewählte Datei konnte nicht gelesen werden.")
            }
            ImportAbort::ChosenNotSshConfig => write!(
                f,
                "Die gewählte Datei ist keine ssh_config. Es wurde nichts angelegt."
            ),
            ImportAbort::TotalBytes => write!(
                f,
                "Die eingelesenen Dateien sind zusammen größer als {} MiB. Es wurde nichts angelegt.",
                MAX_TOTAL_BYTES / (1024 * 1024)
            ),
            ImportAbort::TotalLines => write!(
                f,
                "Die eingelesenen Dateien haben zusammen mehr als {MAX_TOTAL_LINES} Zeilen. Es wurde nichts angelegt."
            ),
            ImportAbort::HostBlocks => write!(
                f,
                "Die eingelesenen Dateien enthalten zusammen mehr als {MAX_HOST_BLOCKS} Host-Blöcke. Es wurde nichts angelegt."
            ),
            // Bewusst ohne den Wert — nur Datei und Zeile (§3.1.5, §5.4).
            ImportAbort::ValueTooLong { file, line } => write!(
                f,
                "{file}, Zeile {line}: Ein einzelner Wert ist länger als erlaubt. Es wurde nichts angelegt."
            ),
            ImportAbort::NoHostBlock => write!(
                f,
                "In den eingelesenen Dateien steht kein Host-Block. Es wurde nichts angelegt."
            ),
        }
    }
}

/// Das Ergebnis des Lesens — Eingabe für `core::profiles::ssh_config::build_plan`.
#[derive(Debug, Clone, Default)]
pub struct ImportRead {
    pub sources: Vec<ImportSource>,
    /// In Lesereihenfolge (§3.1.4).
    pub files: Vec<FileReport>,
    /// `Include`-Befunde, die zu den Meldungen des Parsers dazukommen.
    pub include_issues: Vec<SkippedReport>,
}

/// Liest die gewählte Datei und alles, was sie über `Include` einbindet.
///
/// Der Pfad kommt aus einem Dateidialog, den das Backend selbst geöffnet
/// hat — nie aus dem Frontend (dieselbe Regel wie bei
/// `commands::read_credential_file`, Spec 0013/SEC-06).
pub fn read_import(chosen: &Path) -> Result<ImportRead, ImportAbort> {
    let mut w = Walker::default();
    w.visit(chosen, 0, None, true)?;

    // §3.1.13: in keiner gelesenen Datei ein `Host`-Block.
    if w.out.sources.iter().all(|s| s.parsed.blocks.is_empty()) {
        return Err(ImportAbort::NoHostBlock);
    }
    Ok(w.out)
}

#[derive(Default)]
struct Walker {
    out: ImportRead,
    /// §3.1.4: „Jede Datei wird je Import höchstens einmal gelesen;
    /// entschieden über den aufgelösten, kanonischen Pfad." Das erschlägt
    /// den Selbstbezug **und** die Kette über mehrere Dateien — und, weil
    /// `canonicalize` symbolischen Links folgt, auch den Umweg über einen
    /// Link auf eine schon gelesene Datei.
    visited: HashSet<PathBuf>,
    bytes: u64,
    lines: u64,
    blocks: usize,
}

impl Walker {
    fn visit(
        &mut self,
        path: &Path,
        depth: u8,
        parent: Option<usize>,
        is_chosen: bool,
    ) -> Result<(), ImportAbort> {
        // Kanonisch, bevor irgendetwas gelesen wird: Nur so ist die
        // Besuchtmenge belastbar.
        let canon = match std::fs::canonicalize(path) {
            Ok(c) => c,
            Err(_) => {
                if is_chosen {
                    return Err(ImportAbort::ChosenUnreadable);
                }
                self.report(path, depth, FileStatus::Unreadable);
                return Ok(());
            }
        };
        let shown = canon.display().to_string();

        if !self.visited.insert(canon.clone()) {
            // Zweites Vorkommen: überspringen und melden (§3.1.4).
            self.out.files.push(FileReport {
                path: shown,
                depth,
                status: FileStatus::AlreadyRead,
            });
            return Ok(());
        }

        // §3.3: gedeckelt lesen, damit eine riesige Datei keinen
        // Speicherdruck macht, bevor die Grenze überhaupt greift.
        let remaining = MAX_TOTAL_BYTES.saturating_sub(self.bytes);
        let bytes = match read_capped(&canon, remaining) {
            Ok(b) => b,
            Err(_) => {
                if is_chosen {
                    return Err(ImportAbort::ChosenUnreadable);
                }
                self.out.files.push(FileReport {
                    path: shown,
                    depth,
                    status: FileStatus::Unreadable,
                });
                return Ok(());
            }
        };
        if bytes.len() as u64 > remaining {
            return Err(ImportAbort::TotalBytes);
        }
        self.bytes += bytes.len() as u64;

        // §4.2 und §9/Q-1 Punkt 6: „Die Grenzen aus §3.3 greifen weiter
        // **vor** dem Parser." Für die Zeilengrenze galt das vorher nicht —
        // eine 8-MiB-Datei mit Millionen kurzer unbekannter Zeilen wurde
        // erst vollständig geparst und baute dabei Millionen Meldungen auf,
        // bevor die Grenze feuerte (Review-Runde 1). Hier gezählt, ohne zu
        // parsen.
        let newlines = bytes.iter().filter(|b| **b == b'\n').count() as u64;
        if self.lines + newlines + 1 > MAX_TOTAL_LINES {
            return Err(ImportAbort::TotalLines);
        }

        let parsed = match parse_source(&bytes) {
            Err(too_long) => {
                return Err(ImportAbort::ValueTooLong {
                    file: shown,
                    line: too_long.line,
                })
            }
            Ok(FileParse::NotSshConfig) => {
                // §3.1.4a: Für die **gewählte** Datei greift §3.1.13, für
                // eine eingebundene das Überspringen — mit Pfad, ohne
                // Inhalt. Das ist die Stelle, an der der Anzeige-Kanal aus
                // §5.4 zugeht.
                if is_chosen {
                    return Err(ImportAbort::ChosenNotSshConfig);
                }
                self.out.files.push(FileReport {
                    path: shown,
                    depth,
                    status: FileStatus::NotSshConfig,
                });
                return Ok(());
            }
            Ok(FileParse::Parsed(p)) => p,
        };

        self.lines += u64::from(parsed.line_count);
        if self.lines > MAX_TOTAL_LINES {
            return Err(ImportAbort::TotalLines);
        }
        self.blocks += parsed.blocks.len();
        if self.blocks > MAX_HOST_BLOCKS {
            return Err(ImportAbort::HostBlocks);
        }

        let includes = parsed.includes.clone();
        self.out.files.push(FileReport {
            path: shown.clone(),
            depth,
            status: FileStatus::Read,
        });
        let me = self.out.sources.len();
        self.out.sources.push(ImportSource {
            path: shown.clone(),
            parent,
            parsed,
        });

        // Tiefenschnitt in Lesereihenfolge, wie OpenSSH es täte: ein
        // `Include` wird an seiner Stelle eingesetzt. Damit steht die
        // einbindende Datei immer **vor** der eingebundenen, was §4.5 für
        // den Gruppenbaum und §3.1.11 für `groups.parent_id` braucht.
        let dir = canon.parent().unwrap_or(Path::new("/")).to_path_buf();
        for inc in includes {
            if depth >= MAX_INCLUDE_DEPTH {
                self.issue(&shown, inc.line, SkipReason::IncludeTooDeep);
                continue;
            }
            match resolve_include(&inc.raw, &dir) {
                Ok(targets) if targets.is_empty() => {
                    self.issue(&shown, inc.line, SkipReason::IncludeNoMatch);
                }
                Ok(targets) => {
                    for t in targets {
                        self.visit(&t, depth + 1, Some(me), false)?;
                    }
                }
                Err(()) => self.issue(&shown, inc.line, SkipReason::IncludeNoMatch),
            }
        }
        Ok(())
    }

    fn report(&mut self, path: &Path, depth: u8, status: FileStatus) {
        self.out.files.push(FileReport {
            path: path.display().to_string(),
            depth,
            status,
        });
    }

    fn issue(&mut self, file: &str, line: u32, reason: SkipReason) {
        self.out.include_issues.push(SkippedReport {
            file: file.to_string(),
            line,
            directive: SkippedKind::Directive("Include".to_string()),
            reason,
            entry: None,
        });
    }
}

/// Liest höchstens `remaining + 1` Bytes — ein Byte mehr als erlaubt, damit
/// ein Überschreiten erkennbar ist, **ohne** die ganze Datei zu laden
/// (§5.6: eine überlange Datei führt zu einer Meldung, nicht zu
/// Speicherdruck).
fn read_capped(path: &Path, remaining: u64) -> std::io::Result<Vec<u8>> {
    let f = std::fs::File::open(path)?;
    let mut buf = Vec::new();
    f.take(remaining.saturating_add(1)).read_to_end(&mut buf)?;
    Ok(buf)
}

fn has_wildcard(s: &str) -> bool {
    s.contains('*') || s.contains('?') || s.contains('[')
}

/// Löst einen `Include`-Wert auf (§3.1.4).
///
/// - **Pfade werden nicht eingeschränkt**: absolut, `~` und `..` sind
///   zulässig (E-4).
/// - Ein relativer Pfad gilt relativ zum **Verzeichnis der einbindenden
///   Datei** — nicht relativ zu `~/.ssh`, wie OpenSSH es für die
///   Nutzerkonfiguration tut, weil die gewählte Datei eben nicht
///   `~/.ssh/config` sein muss. Genau hier lag einer der Messbefunde gegen
///   `ssh2-config` (§9/M-1): Sie löst gegen `$HOME/.ssh` auf und läse damit
///   das echte SSH-Verzeichnis des Nutzers.
/// - Platzhalter werden aufgelöst; jede getroffene Datei zählt einzeln.
///
/// ANNAHME A-2 (Q-BL-0216-01): Ein Platzhalter wird nur in der **letzten**
/// Pfadkomponente aufgelöst (`conf.d/*.conf` — der Fall, den §3.1.4 nennt
/// und §6.2.3 prüft). Steht einer weiter vorne, wird die Zeile gemeldet
/// statt das Dateisystem breit abzusuchen: Ein Muster wie `/*/*/*` aus
/// einer fremden Datei würde sonst einen Verzeichnisbaum durchlaufen, den
/// niemand begrenzt hat — das liefe §5.6 zuwider.
fn resolve_include(raw: &str, including_dir: &Path) -> Result<Vec<PathBuf>, ()> {
    if raw.is_empty() {
        return Err(());
    }

    // `~` und `~/…` auflösen. Die `~user/…`-Form können wir nicht auflösen;
    // sie bleibt stehen und scheitert dann beim Lesen — gemeldet, nicht
    // geraten.
    let expanded: PathBuf = if raw == "~" {
        match home_dir() {
            Some(h) => h,
            None => return Err(()),
        }
    } else if let Some(rest) = raw.strip_prefix("~/") {
        match home_dir() {
            Some(h) => h.join(rest),
            None => return Err(()),
        }
    } else if raw.starts_with('/') {
        PathBuf::from(raw)
    } else {
        including_dir.join(raw)
    };

    let text = expanded.to_string_lossy().to_string();
    if !has_wildcard(&text) {
        return Ok(vec![expanded]);
    }

    let (dir, last) = match expanded.file_name() {
        Some(name) => (
            expanded.parent().unwrap_or(Path::new("/")).to_path_buf(),
            name.to_string_lossy().to_string(),
        ),
        None => return Err(()),
    };
    // Platzhalter nur in der letzten Komponente (A-2).
    if has_wildcard(&dir.to_string_lossy()) {
        return Err(());
    }

    let glob = globset::Glob::new(&last).map_err(|_| ())?.compile_matcher();
    let mut hits: Vec<PathBuf> = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };
    for e in entries.flatten() {
        let name = e.file_name();
        if glob.is_match(Path::new(&name)) {
            hits.push(e.path());
        }
    }
    // Verzeichnisreihenfolge ist keine Reihenfolge — sortieren, damit
    // derselbe Import zweimal dasselbe Ergebnis hat (§3.1.10).
    hits.sort();
    Ok(hits)
}

fn home_dir() -> Option<PathBuf> {
    // `directories` ist schon Abhängigkeit von `app-shell`; sein
    // `UserDirs::home_dir` ist derselbe Wert, den der Rest der App benutzt.
    directories::UserDirs::new().map(|u| u.home_dir().to_path_buf())
}

#[cfg(test)]
mod tests;
