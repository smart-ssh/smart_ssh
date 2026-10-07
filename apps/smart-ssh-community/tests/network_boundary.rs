//! Issue #14: Die App baut keine Netzwerkverbindung auf, die der Nutzer
//! nicht ausgelöst hat. Welche Verbindungen es gibt, listet
//! `docs/netzwerkverbindungen.md`. Diese Tests verhindern, dass eine neue
//! Verbindungsart unbemerkt dazukommt:
//!
//! 1. Die CSP des Webviews (Issue #62): Jede Direktive steht genau einmal
//!    in der CSP und hat genau die Quellen aus [`ALLOWED_CSP`]. Damit kann
//!    keine Direktive unbemerkt eine entfernte Herkunft erlauben, weder
//!    `connect-src` (`fetch`/XHR/WebSocket nur an die App selbst und die
//!    Tauri-IPC) noch `script-src`, `style-src`, `img-src` oder der
//!    Rückfall `default-src`, der für alle nicht gesetzten Direktiven gilt
//!    (Schriften, Medien, Frames, Worker …). Eine Direktive ohne Eintrag in
//!    der Liste (z. B. ein neues `font-src`) und ein fehlendes
//!    `default-src` (Browser-Vorgabe: alles erlaubt) lassen den Test
//!    scheitern.
//! 2. Nur eine feste Liste von Crates hängt direkt an einem HTTP-Client.
//!    Geprüft wird der Produktionsgraph (`cargo tree -e normal,build`, ohne
//!    Dev-Abhängigkeiten) für die Plattform, auf der der Test läuft. CI
//!    deckt damit Linux, Windows und macOS ab.
//!
//! Wer eine dieser Listen erweitert, trägt die neue Verbindung zuerst in
//! `docs/netzwerkverbindungen.md` ein.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Erlaubte Quellen in `connect-src`. Reihenfolge egal, Menge exakt.
const ALLOWED_CONNECT_SRC: &[&str] = &["'self'", "ipc:"];

/// Jede erlaubte CSP-Direktive mit genau ihren erlaubten Quellen.
/// Reihenfolge egal, Menge exakt. Jede Direktive hier muss genau einmal in
/// der CSP stehen; eine Direktive, die hier fehlt, ist verboten.
///
/// Die `tauri-plugin-decoration`-Quellen in `style-src` zeigen auf ein
/// lokales Custom-Protocol (`*.localhost`), nicht ins Netz.
const ALLOWED_CSP: &[(&str, &[&str])] = &[
    ("default-src", &["'self'"]),
    ("script-src", &["'self'"]),
    (
        "style-src",
        &[
            "'self'",
            "'unsafe-inline'",
            "tauri-plugin-decoration:",
            "http://tauri-plugin-decoration.localhost",
            "https://tauri-plugin-decoration.localhost",
        ],
    ),
    ("img-src", &["'self'", "data:"]),
    ("connect-src", ALLOWED_CONNECT_SRC),
];

/// Crates, über die ein Programm HTTP-Anfragen stellen kann. `hyper`,
/// `hyper-util` und `h2` stehen mit drin, weil sie sowohl Client als auch
/// Server sind: Wer sie direkt einbindet, muss begründen, wofür.
const HTTP_STACK_CRATES: &[&str] = &[
    "attohttpc",
    "awc",
    "curl",
    "h2",
    "http_req",
    "hyper",
    "hyper-rustls",
    "hyper-tls",
    "hyper-util",
    "isahc",
    "minreq",
    "reqwest",
    "surf",
    "tauri-plugin-http",
    "tauri-plugin-updater",
    "tokio-tungstenite",
    "tungstenite",
    "ureq",
];

/// Die einzigen erlaubten Kanten "Crate -> HTTP-Crate" im Produktionsgraph.
/// Kanten zwischen zwei HTTP-Crates (z. B. `reqwest -> hyper`) zählen
/// nicht, sie sind nur über eine der Kanten hier erreichbar.
const ALLOWED_DEPENDENTS: &[(&str, &str)] = &[
    // KI-Provider: Chat, Modellsuche, Schlüsselprüfung, Attestierung.
    ("ai-providers", "reqwest"),
    // Lokaler MCP-Listener (`mcp-server`, nur 127.0.0.1): axum nutzt hyper
    // als Server.
    ("axum", "hyper"),
    ("axum", "hyper-util"),
];

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn workspace_root() -> PathBuf {
    manifest_dir()
        .join("../..")
        .canonicalize()
        .expect("Workspace-Wurzel auflösbar")
}

// ---------------------------------------------------------------------------
// CSP
// ---------------------------------------------------------------------------

fn configured_csp() -> String {
    let path = manifest_dir().join("tauri.conf.json");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} nicht lesbar: {e}", path.display()));
    let conf: serde_json::Value = serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("{} ist kein JSON: {e}", path.display()));
    conf["app"]["security"]["csp"]
        .as_str()
        .expect("app.security.csp ist als String gesetzt")
        .to_owned()
}

/// Prüft die ganze CSP gegen [`ALLOWED_CSP`]: Jede Direktive dort steht
/// genau einmal in `csp` und hat genau die erlaubten Quellen (doppelte
/// Quellen zählen als Änderung), und `csp` enthält keine weitere Direktive.
/// Fehlt `connect-src`, fiele der Webview auf `default-src` zurück; fehlt
/// `default-src`, erlaubt der Browser alles. Beides zählt als Fehler.
/// Alle gefundenen Abweichungen landen in der Fehlermeldung.
fn check_csp(csp: &str) -> Result<(), String> {
    let directives: Vec<Vec<&str>> = csp
        .split(';')
        .map(|d| d.split_whitespace().collect::<Vec<_>>())
        .filter(|d| !d.is_empty())
        .collect();
    let mut errors = Vec::new();

    for directive in &directives {
        let name = directive[0];
        if !ALLOWED_CSP
            .iter()
            .any(|(allowed, _)| allowed.eq_ignore_ascii_case(name))
        {
            errors.push(format!(
                "Direktive {name} ist nicht erlaubt (Quellen: {:?})",
                &directive[1..]
            ));
        }
    }

    for (name, allowed) in ALLOWED_CSP {
        let found: Vec<&Vec<&str>> = directives
            .iter()
            .filter(|d| d[0].eq_ignore_ascii_case(name))
            .collect();
        let [directive] = found.as_slice() else {
            errors.push(format!(
                "erwartet genau eine {name}-Direktive, gefunden: {}",
                found.len()
            ));
            continue;
        };
        let actual: BTreeSet<&str> = directive[1..].iter().copied().collect();
        let expected: BTreeSet<&str> = allowed.iter().copied().collect();
        if actual.len() != directive.len() - 1 || actual != expected {
            errors.push(format!(
                "{name} ist {:?}, erlaubt ist genau {allowed:?}",
                &directive[1..]
            ));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

#[test]
fn csp_allows_only_allowlisted_sources() {
    let csp = configured_csp();
    if let Err(e) = check_csp(&csp) {
        panic!(
            "{e}\nCSP: {csp}\nEine neue Verbindung oder Quelle aus dem Webview gehört \
             erst in docs/netzwerkverbindungen.md und dann in ALLOWED_CSP \
             (connect-src: ALLOWED_CONNECT_SRC)."
        );
    }
}

/// Die aktuelle CSP aus `tauri.conf.json`, als Ausgangspunkt der Tests.
const CURRENT_CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' \
    'unsafe-inline' tauri-plugin-decoration: http://tauri-plugin-decoration.localhost \
    https://tauri-plugin-decoration.localhost; img-src 'self' data:; connect-src 'self' ipc:;";

#[test]
fn csp_check_accepts_current_values_in_any_order() {
    assert!(check_csp(CURRENT_CSP).is_ok());
    // Direktiven umgestellt, Quellen umgestellt, Groß-/Kleinschreibung der
    // Namen, ohne abschließendes Semikolon.
    let reordered = "CONNECT-SRC ipc: 'self'; img-src data: 'self'; \
        style-src https://tauri-plugin-decoration.localhost 'unsafe-inline' \
        http://tauri-plugin-decoration.localhost tauri-plugin-decoration: 'self'; \
        script-src 'self'; default-src 'self'";
    assert_eq!(check_csp(reordered), Ok(()));
}

#[test]
fn connect_src_check_rejects_every_change() {
    let base = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline' \
        tauri-plugin-decoration: http://tauri-plugin-decoration.localhost \
        https://tauri-plugin-decoration.localhost; img-src 'self' data:;";
    assert!(check_csp(&format!("{base} connect-src 'self' ipc:;")).is_ok());
    assert!(check_csp(&format!("{base} connect-src ipc: 'self'")).is_ok());

    for changed in [
        // zusätzliche Quelle
        "connect-src 'self' ipc: https://example.com;",
        "connect-src 'self' ipc: http://127.0.0.1:11434;",
        "connect-src 'self' ipc: *;",
        "connect-src 'self' ipc: https:;",
        // fehlende Quelle
        "connect-src 'self';",
        // doppelte Quelle verdeckt keine Änderung
        "connect-src 'self' 'self' ipc:;",
        // Direktive fehlt (Rückfall auf default-src)
        "",
        // zweite Direktive
        "connect-src 'self' ipc:; connect-src https://example.com;",
    ] {
        let csp = format!("{base} {changed}");
        assert!(
            check_csp(&csp).is_err(),
            "geänderte CSP wurde akzeptiert: {csp}"
        );
    }
}

/// Ersetzt in [`CURRENT_CSP`] die Direktive `name` durch `replacement`
/// (leer: Direktive entfernt).
fn csp_with(name: &str, replacement: &str) -> String {
    let directives: Vec<String> = CURRENT_CSP
        .split(';')
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(|d| {
            if d.split_whitespace().next() == Some(name) {
                replacement.to_owned()
            } else {
                d.to_owned()
            }
        })
        .filter(|d| !d.is_empty())
        .collect();
    assert_ne!(
        directives.join("; "),
        CURRENT_CSP.trim_end_matches(';'),
        "{name} kommt in CURRENT_CSP nicht vor"
    );
    directives.join("; ")
}

#[test]
fn csp_check_rejects_remote_origins_in_every_directive() {
    let style_remote = format!(
        "{} https://fonts.googleapis.com",
        CURRENT_CSP
            .split(';')
            .map(str::trim)
            .find(|d| d.starts_with("style-src"))
            .expect("style-src in CURRENT_CSP")
    );
    let changed = [
        csp_with("img-src", "img-src 'self' data: https:"),
        csp_with("img-src", "img-src 'self' data: https://example.com"),
        csp_with("script-src", "script-src 'self' https://cdn.example.com"),
        csp_with("style-src", &style_remote),
        csp_with("default-src", "default-src *"),
        csp_with("default-src", "default-src 'self' https:"),
        // fehlende Quelle
        csp_with("img-src", "img-src 'self'"),
        // doppelte Quelle verdeckt keine Änderung
        csp_with("script-src", "script-src 'self' 'self'"),
        // Direktive ohne Eintrag in ALLOWED_CSP
        format!("{CURRENT_CSP} font-src https://fonts.gstatic.com;"),
        format!("{CURRENT_CSP} font-src 'self';"),
        format!("{CURRENT_CSP} frame-src https://example.com;"),
        // default-src fehlt (Browser-Vorgabe: alles erlaubt)
        csp_with("default-src", ""),
        csp_with("script-src", ""),
        csp_with("style-src", ""),
        csp_with("img-src", ""),
        // Direktive doppelt, auch mit identischen Quellen
        format!("{CURRENT_CSP} img-src 'self' data:;"),
        format!("{CURRENT_CSP} default-src 'self';"),
        format!("{CURRENT_CSP} IMG-SRC https://example.com;"),
    ];
    for csp in changed {
        assert!(
            check_csp(&csp).is_err(),
            "geänderte CSP wurde akzeptiert: {csp}"
        );
    }
}

// ---------------------------------------------------------------------------
// HTTP-Clients im Abhängigkeitsgraph
// ---------------------------------------------------------------------------

/// Produktionsgraph aller Workspace-Crates für die aktuelle Plattform, eine
/// Zeile je Paket mit vorangestellter Tiefe (`--prefix depth`). Läuft
/// `--offline --locked`: nach dem Build liegen alle Pakete schon lokal, der
/// Test greift nicht aufs Netz zu.
fn production_dependency_tree(root: &Path) -> String {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .current_dir(root)
        .args([
            "tree",
            "--workspace",
            "--edges",
            "normal,build",
            "--prefix",
            "depth",
            "--format",
            "{p}",
            "--locked",
            "--offline",
        ])
        .output()
        .expect("cargo tree startet");
    assert!(
        output.status.success(),
        "cargo tree fehlgeschlagen ({}):\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("cargo tree liefert UTF-8")
}

/// Alle Kanten `Paket -> HTTP-Crate` aus der Ausgabe von
/// [`production_dependency_tree`], ohne Kanten zwischen zwei HTTP-Crates.
fn http_client_edges(tree: &str) -> BTreeSet<(String, String)> {
    let mut path: Vec<&str> = Vec::new();
    let mut edges = BTreeSet::new();
    for line in tree.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            path.clear();
            continue;
        }
        let digits = line.bytes().take_while(u8::is_ascii_digit).count();
        let depth: usize = line[..digits]
            .parse()
            .unwrap_or_else(|_| panic!("Zeile ohne Tiefenangabe: {line:?}"));
        let name = line[digits..]
            .split_whitespace()
            .next()
            .unwrap_or_else(|| panic!("Zeile ohne Paketname: {line:?}"));
        assert!(
            depth <= path.len(),
            "Tiefe springt von {} auf {depth}: {line:?}",
            path.len()
        );
        path.truncate(depth);
        if let Some(parent) = path.last() {
            if HTTP_STACK_CRATES.contains(&name) && !HTTP_STACK_CRATES.contains(parent) {
                edges.insert(((*parent).to_owned(), name.to_owned()));
            }
        }
        path.push(name);
    }
    edges
}

/// Kanten, die nicht in [`ALLOWED_DEPENDENTS`] stehen.
fn disallowed(edges: &BTreeSet<(String, String)>) -> Vec<String> {
    edges
        .iter()
        .filter(|(from, to)| !ALLOWED_DEPENDENTS.iter().any(|(f, t)| f == from && t == to))
        .map(|(from, to)| format!("{from} -> {to}"))
        .collect()
}

#[test]
fn only_allowlisted_crates_depend_on_an_http_client() {
    let tree = production_dependency_tree(&workspace_root());
    let edges = http_client_edges(&tree);

    let unexpected = disallowed(&edges);
    assert!(
        unexpected.is_empty(),
        "Neue Abhängigkeit auf einen HTTP-Client: {unexpected:?}\nJede ausgehende \
         Verbindung gehört erst in docs/netzwerkverbindungen.md und dann in \
         ALLOWED_DEPENDENTS."
    );

    // Jede Ausnahme muss noch gebraucht werden. Das hält die Liste und das
    // Dokument aktuell und beweist nebenbei, dass der Parser die Kanten
    // überhaupt sieht.
    let stale: Vec<String> = ALLOWED_DEPENDENTS
        .iter()
        .filter(|(f, t)| !edges.contains(&((*f).to_owned(), (*t).to_owned())))
        .map(|(f, t)| format!("{f} -> {t}"))
        .collect();
    assert!(
        stale.is_empty(),
        "Ausnahme ohne passende Abhängigkeit, bitte aus ALLOWED_DEPENDENTS und \
         docs/netzwerkverbindungen.md entfernen: {stale:?}"
    );
}

#[test]
fn http_client_edges_catch_direct_and_transitive_additions() {
    let allowed_only = "\
0ai-providers v0.5.2 (/ws/crates/ai-providers)
1reqwest v0.13.4
2hyper v1.8.1
2hyper-rustls v0.27.7
3hyper v1.8.1
0mcp-server v0.5.2 (/ws/crates/mcp-server)
1axum v0.8.9
2hyper v1.8.1
2hyper-util v0.1.20
3hyper v1.8.1
1rmcp v3.2.0
";
    assert!(disallowed(&http_client_edges(allowed_only)).is_empty());

    // Direkt in einer Workspace-Crate.
    let direct = format!("{allowed_only}0app-logic v0.5.2 (/ws/crates/app-logic)\n1ureq v3.0.0\n");
    assert_eq!(
        disallowed(&http_client_edges(&direct)),
        ["app-logic -> ureq"]
    );

    // Über eine Drittanbieter-Crate, auch tief im Baum.
    let transitive = format!(
        "{allowed_only}0app-shell v0.5.2 (/ws/crates/app-shell)\n\
         1tauri-plugin-foo v1.0.0\n2foo-core v1.0.0\n3reqwest v0.13.4\n"
    );
    assert_eq!(
        disallowed(&http_client_edges(&transitive)),
        ["foo-core -> reqwest"]
    );

    // Ein Updater-Plugin direkt in der App.
    let updater = format!(
        "{allowed_only}0smart-ssh-community v0.5.2 (/ws/apps/smart-ssh-community)\n\
         1tauri-plugin-updater v2.0.0 (*)\n"
    );
    assert_eq!(
        disallowed(&http_client_edges(&updater)),
        ["smart-ssh-community -> tauri-plugin-updater"]
    );

    // hyper direkt, z. B. als Client, braucht ebenfalls eine Ausnahme.
    let hyper = format!("{allowed_only}0core v0.5.2 (/ws/crates/core)\n1hyper v1.8.1\n");
    assert_eq!(disallowed(&http_client_edges(&hyper)), ["core -> hyper"]);
}
