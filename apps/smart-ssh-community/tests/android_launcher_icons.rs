//! Issue #980: the Android launcher icons are the Smart SSH logo, not the
//! Tauri template, and `gen/android` carries exactly the files of the single
//! source of truth `icons/android/` (see `docs/android-build.md`).

use std::fs;
use std::path::{Path, PathBuf};

fn app_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// FNV-1a hashes of every PNG of the Tauri template launcher set.
const TEMPLATE_HASHES: [u64; 10] = [
    0xc6ee922c92fac53c,
    0x7f37a052f0d8b529,
    0xae3f17869f4811bc,
    0xfc618bb710253b57,
    0x3bbc1965b10d6536,
    0x9c401c5c6847b597,
    0x107b8b3afeabe3c3,
    0x16c6419270e12b0a,
    0x028ba694396f38ba,
    0xa22213fcb8375157,
];

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            files(&p, out);
        } else {
            out.push(p);
        }
    }
}

fn rel_files(base: &Path) -> Vec<PathBuf> {
    let mut v = Vec::new();
    files(base, &mut v);
    let mut v: Vec<PathBuf> = v
        .into_iter()
        .map(|p| p.strip_prefix(base).unwrap().to_path_buf())
        .collect();
    v.sort();
    v
}

#[test]
fn launcher_pngs_are_the_logo_and_match_the_source_of_truth() {
    let src = app_dir().join("icons/android");
    let res = app_dir().join("gen/android/app/src/main/res");
    let want = rel_files(&src);
    assert!(
        want.iter()
            .filter(|p| p.extension().is_some_and(|e| e == "png"))
            .count()
            == 15,
        "expected 3 PNGs for each of 5 densities: {want:?}"
    );
    for rel in &want {
        let a = fs::read(src.join(rel)).unwrap();
        let b = fs::read(res.join(rel))
            .unwrap_or_else(|_| panic!("{} missing in gen/android res", rel.display()));
        assert_eq!(a, b, "{} differs from icons/android", rel.display());
        if rel.extension().is_some_and(|e| e == "png") {
            assert!(
                !TEMPLATE_HASHES.contains(&fnv1a(&b)),
                "{} is the Tauri template icon",
                rel.display()
            );
        }
    }
    // gen/android/res must not carry additional launcher assets.
    for rel in rel_files(&res) {
        let s = rel.to_string_lossy();
        if s.contains("ic_launcher") {
            assert!(want.contains(&rel), "unexpected launcher asset {s}");
        }
    }
    // Template drawables are gone.
    assert!(!res.join("drawable/ic_launcher_background.xml").exists());
    assert!(!res.join("drawable-v24/ic_launcher_foreground.xml").exists());
}

#[test]
fn adaptive_icon_uses_dark_background_and_mark_foreground() {
    let res = app_dir().join("gen/android/app/src/main/res");
    for name in ["ic_launcher.xml", "ic_launcher_round.xml"] {
        let x = fs::read_to_string(res.join("mipmap-anydpi-v26").join(name)).unwrap();
        assert!(x.contains("<adaptive-icon"), "{name}");
        assert!(x.contains("@color/ic_launcher_background"), "{name}");
        assert!(x.contains("@mipmap/ic_launcher_foreground"), "{name}");
    }
    let bg = fs::read_to_string(res.join("values/ic_launcher_background.xml")).unwrap();
    assert!(bg.contains("#1e293b"), "{bg}");
    assert!(!bg.to_lowercase().contains("#3dda84") && !bg.contains("#fff<"));
}
