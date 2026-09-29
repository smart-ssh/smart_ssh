//! Bettet unter Windows ein Manifest mit der Abhängigkeit auf Common
//! Controls v6 in das Test-Ziel dieser Lib ein (Spec 0093, A2–A4).
//!
//! Ohne dieses Manifest lädt Windows für ein Programm ohne Manifest die
//! alte comctl32-Version, die `TaskDialogIndirect` nicht kennt (kommt aus
//! `rfd`s `common-controls-v6`-Feature, s. `Cargo.toml`) — das Unit-Test-
//! Binary dieser Lib startet dann gar nicht erst
//! (`STATUS_ENTRYPOINT_NOT_FOUND`). Das App-Binary
//! (`apps/smart-ssh-community`) bekommt sein eigenes Manifest bereits über
//! `tauri_build::build()` in dessen eigenem `build.rs` — unverändert, denn
//! `cargo:rustc-link-arg` (ohne `-bins`) wirkt nur auf die Ziele *dieser*
//! Crate (hier: nur das Test-Ziel der Lib, da diese Crate kein `[[bin]]`
//! hat), nicht auf abhängige Pakete. `-bins` scheidet aus (Ist-Stand 4):
//! `embed-manifest` gibt genau das aus, und bricht auf einer Lib ohne
//! Bin-Ziel ab.
//!
//! Die Plattformwahl liest `CARGO_CFG_TARGET_OS`/`CARGO_CFG_TARGET_ENV`
//! (das Build-*Ziel*), nicht `cfg!`/`#[cfg]` (die gälten für den
//! Build-*Rechner* — bei Cross-Kompilierung ein Unterschied).

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Common-Controls-v6-Manifest, wie es für `TaskDialogIndirect` gebraucht
/// wird — dieselbe Abhängigkeit, die `tauri_build`/`rfd`s
/// `common-controls-v6`-Feature für das App-Binary einträgt.
const MANIFEST_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*" />
    </dependentAssembly>
  </dependency>
</assembly>
"#;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() != "windows" {
        return;
    }
    match env::var("CARGO_CFG_TARGET_ENV")
        .unwrap_or_default()
        .as_str()
    {
        "msvc" => embed_via_linker(),
        "gnu" => embed_via_windres(),
        other => panic!("unbekannte Windows-target_env {other:?} — weder msvc noch gnu"),
    }
}

/// MSVC: `link.exe` baut das Manifest selbst und bettet es ein, wenn man
/// ihm die zusätzliche Abhängigkeit mitgibt — kein Ressourcen-Compiler
/// nötig. Nicht per CI-Lauf nachgemessen (Spec 0093, Ist-Stand 4), aber
/// derselbe Import wie beim per `windres` gemessenen GNU-Weg.
fn embed_via_linker() {
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!(
        "cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' \
         name='Microsoft.Windows.Common-Controls' version='6.0.0.0' \
         processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
    );
}

/// GNU (mingw): `ld` kennt kein `/MANIFEST`; das Manifest muss als
/// `.rsrc`-Ressource (Typ 24 = `RT_MANIFEST`, ID 1 =
/// `CREATEPROCESS_MANIFEST_RESOURCE_ID`) mitkompiliert werden — gemessen
/// per Cross-Build/`windres` (Spec 0093, Ist-Stand 4: danach hat das
/// Test-Binary einen `.rsrc`-Abschnitt, vorher keinen).
fn embed_via_windres() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR sollte immer gesetzt sein"));
    let manifest_path = out_dir.join("common-controls-v6.manifest");
    let rc_path = out_dir.join("common-controls-v6.rc");
    let obj_path = out_dir.join("common-controls-v6.o");

    fs::write(&manifest_path, MANIFEST_XML)
        .expect("Manifest-Datei sollte im OUT_DIR schreibbar sein");
    fs::write(&rc_path, format!("1 24 \"{}\"\n", manifest_path.display()))
        .expect("Ressourcen-Skript sollte im OUT_DIR schreibbar sein");

    let windres = windres_binary();
    let status = Command::new(windres)
        .args([
            "--input",
            rc_path
                .to_str()
                .expect("OUT_DIR-Pfad sollte gültiges UTF-8 sein"),
            "--output",
            obj_path
                .to_str()
                .expect("OUT_DIR-Pfad sollte gültiges UTF-8 sein"),
            "--output-format=coff",
        ])
        .status();
    match status {
        Ok(status) if status.success() => {}
        Ok(status) => panic!("{windres} scheiterte mit {status}"),
        Err(err) => panic!("{windres} nicht aufrufbar: {err}"),
    }

    println!("cargo:rustc-link-arg={}", obj_path.display());
}

fn windres_binary() -> &'static str {
    match env::var("CARGO_CFG_TARGET_ARCH")
        .unwrap_or_default()
        .as_str()
    {
        "x86_64" => "x86_64-w64-mingw32-windres",
        "x86" => "i686-w64-mingw32-windres",
        _ => "windres",
    }
}
