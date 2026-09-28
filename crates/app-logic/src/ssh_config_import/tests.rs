//! Tests zu Spec 0075, Schritt 2 (§6.2, §6.4.5, §6.4.9).

use std::fs;
use std::path::{Path, PathBuf};

use ssh_manager_core::profiles::ssh_config::{build_plan, Inventory, SkipReason};
use ssh_manager_core::shared::ServerId;
use uuid::Uuid;

use super::*;

fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    let p = dir.join(name);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).expect("mkdir");
    }
    fs::write(&p, text).expect("write");
    p
}

/// Der Plan zu einem gelesenen Import — §4.5 (Gruppenbaum) hängt daran.
fn plan_of(read: &ImportRead) -> ssh_manager_core::profiles::ssh_config::ImportPlan {
    build_plan(
        &read.sources,
        Inventory {
            servers: &[],
            groups: &[],
            rules: &[],
            local_server_id: ServerId(Uuid::nil()),
        },
    )
}

fn read_ok(chosen: &Path) -> ImportRead {
    read_import(chosen).expect("Import sollte durchlaufen")
}

/// `ImportRead` trägt bewusst kein `PartialEq` (es hängt an den
/// `core`-Typen); der Abbruchgrund wird deshalb einzeln geprüft.
fn expect_abort(chosen: &Path) -> ImportAbort {
    match read_import(chosen) {
        Err(a) => a,
        Ok(r) => panic!("Abbruch erwartet, kam durch: {r:?}"),
    }
}

fn group_names(plan: &ssh_manager_core::profiles::ssh_config::ImportPlan) -> Vec<String> {
    plan.groups.iter().map(|g| g.name.clone()).collect()
}

// ------------------------------------------------------------- §6.2

#[test]
fn t_6_2_1_datei_ohne_include_ergibt_eine_gruppe() {
    let d = tempfile::tempdir().unwrap();
    let c = write(d.path(), "config", "Host a\n  HostName 1.1.1.1\nHost b\n");
    let read = read_ok(&c);
    let plan = plan_of(&read);
    assert_eq!(group_names(&plan), vec!["config"]);
    assert_eq!(plan.entries.len(), 2);
    for e in &plan.entries {
        assert_eq!(e.group, 0);
    }
}

#[test]
fn t_6_2_2_verschachtelte_includes_ergeben_die_kette() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "kunden.conf", "Host c\n");
    write(d.path(), "team.conf", "Include kunden.conf\nHost b\n");
    let c = write(d.path(), "config", "Include team.conf\nHost a\n");

    let read = read_ok(&c);
    let plan = plan_of(&read);
    assert_eq!(
        group_names(&plan),
        vec!["config", "team.conf", "kunden.conf"]
    );
    assert_eq!(plan.groups[0].parent, None);
    assert_eq!(plan.groups[1].parent, Some(0));
    assert_eq!(plan.groups[2].parent, Some(1));
    // Jeder Server in der Gruppe seiner Herkunftsdatei (§4.5).
    let g = |n: &str| plan.entries.iter().find(|e| e.name == n).unwrap().group;
    assert_eq!(g("a"), 0);
    assert_eq!(g("b"), 1);
    assert_eq!(g("c"), 2);
}

#[test]
fn t_6_2_3_platzhalter_im_include_pfad() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "conf.d/one.conf", "Host one\n");
    write(d.path(), "conf.d/two.conf", "Host two\n");
    write(d.path(), "conf.d/three.conf", "Host three\n");
    // Trifft das Muster nicht — darf nicht gelesen werden.
    write(d.path(), "conf.d/ignoriert.txt", "Host nichtdabei\n");
    let c = write(d.path(), "config", "Include conf.d/*.conf\nHost a\n");

    let read = read_ok(&c);
    let plan = plan_of(&read);
    // Drei Untergruppen, plus die Gruppe der gewählten Datei.
    assert_eq!(plan.groups.len(), 4);
    assert_eq!(plan.groups[0].name, "config");
    let subs: Vec<&String> = plan.groups[1..].iter().map(|g| &g.name).collect();
    assert_eq!(subs, vec!["one.conf", "three.conf", "two.conf"], "sortiert");
    for g in &plan.groups[1..] {
        assert_eq!(g.parent, Some(0));
    }
    let names: Vec<&str> = plan.entries.iter().map(|e| e.name.as_str()).collect();
    assert!(!names.contains(&"nichtdabei"), "Nicht-Treffer gelesen");
}

// ---------------------------- ANNAHME A-2 (ADR 0074 Punkt 5/8, §9 Spec)

// spec-reviewer-Fund (Runde 1, S-1): `*` ist auf Windows kein gültiges
// Pfadzeichen — `create_dir_all` auf ein Verzeichnis, das buchstäblich `*`
// heißt, schlägt dort mit `ERROR_INVALID_NAME` fehl, und `.expect("mkdir")`
// paniert. Der CI-Workflow fährt `cargo test --workspace` auch unter
// `windows-latest` (`.github/workflows/community.yml`). Die geprüfte
// Produktionslogik (`has_wildcard(&dir)`) ist plattformunabhängig; nur der
// Gegenbeweis-Aufbau dieses Tests braucht ein Unix-Dateisystem — deshalb
// `#[cfg(unix)]`, wie an den anderen Stellen dieses Repos, die ein
// Sonderzeichen im Dateinamen brauchen (z. B.
// `ssh_config_export/tests.rs::t_review1_symlink_auf_die_datei_selbst_wird_erkannt`).
#[cfg(unix)]
#[test]
fn t_a2_platzhalter_vor_letzter_pfadkomponente_wird_gemeldet_nicht_aufgeloest() {
    // ANNAHME A-2, bestätigt (Architekt, 2026-09-25, §9 der Spec): Ein
    // Platzhalter weiter vorne als die letzte Pfadkomponente
    // (`Include */conf.d/x.conf`) wird gemeldet, nicht aufgelöst — sonst
    // liefe ein Muster aus fremder Hand einen unbegrenzten Verzeichnisbaum
    // ab (§5.6). `resolve_include` prüft das vor jedem Dateizugriff
    // (`has_wildcard(&dir…)`).
    //
    // Damit der Gegenbeweis überhaupt etwas beweist, muss ein Treffer
    // *möglich* sein, wenn die Prüfung fehlte: Auf Unix ist `*` ein
    // gültiges Zeichen in einem Dateinamen, also legen wir ein
    // Verzeichnis an, das buchstäblich `*` heißt — genau das Verzeichnis,
    // das `*/conf.d/x.conf` treffen würde, gäbe es die Prüfung nicht.
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "*/conf.d/x.conf", "Host getroffen\n");
    let c = write(d.path(), "config", "Include */conf.d/x.conf\nHost a\n");

    let read = read_ok(&c);
    let plan = plan_of(&read);
    let names: Vec<&str> = plan.entries.iter().map(|e| e.name.as_str()).collect();
    assert!(
        !names.contains(&"getroffen"),
        "Platzhalter vor der letzten Pfadkomponente wurde aufgelöst: {names:?}"
    );
    assert!(
        read.include_issues
            .iter()
            .any(|i| i.reason == SkipReason::IncludeNoMatch),
        "nicht gemeldet: {:?}",
        read.include_issues
    );
}

#[test]
fn t_6_2_4_relativer_include_relativ_zur_einbindenden_datei() {
    // Der Messbefund gegen `ssh2-config` (§9/M-1) als Test: Sie löst
    // relative Pfade gegen `$HOME/.ssh` auf. Hier muss das Verzeichnis der
    // **einbindenden** Datei gewinnen — nicht `~/.ssh`, nicht das
    // Arbeitsverzeichnis.
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "unten/tief.conf", "Host tief\n");
    let c = write(d.path(), "unten/config", "Include tief.conf\nHost oben\n");

    let read = read_ok(&c);
    let names: Vec<&str> = read
        .files
        .iter()
        .filter(|f| f.status == FileStatus::Read)
        .map(|f| f.path.as_str())
        .collect();
    assert_eq!(names.len(), 2, "beide Dateien gelesen: {names:?}");
    assert!(names[1].ends_with("unten/tief.conf"), "war: {}", names[1]);
    let plan = plan_of(&read);
    assert!(plan.entries.iter().any(|e| e.name == "tief"));
}

#[test]
fn t_6_2_5_absoluter_pfad_und_punkt_punkt_werden_gefolgt() {
    let d = tempfile::tempdir().unwrap();
    let abs = write(d.path(), "abs.conf", "Host viaabs\n");
    write(d.path(), "nachbar/rel.conf", "Host viarel\n");
    let c = write(
        d.path(),
        "start/config",
        &format!(
            "Include {}\nInclude ../nachbar/rel.conf\nHost a\n",
            abs.display()
        ),
    );

    let read = read_ok(&c);
    let plan = plan_of(&read);
    let names: Vec<&str> = plan.entries.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"viaabs"), "absolut: {names:?}");
    assert!(names.contains(&"viarel"), "über ..: {names:?}");
}

#[test]
fn t_6_2_6_include_auf_tiefe_3_wird_nicht_mehr_gefolgt() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "d4.conf", "Host vier\n");
    write(d.path(), "d3.conf", "Include d4.conf\nHost drei\n");
    write(d.path(), "d2.conf", "Include d3.conf\nHost zwei\n");
    write(d.path(), "d1.conf", "Include d2.conf\nHost eins\n");
    let c = write(d.path(), "config", "Include d1.conf\nHost null\n");

    let read = read_ok(&c);
    let plan = plan_of(&read);
    let names: Vec<&str> = plan.entries.iter().map(|e| e.name.as_str()).collect();
    // Tiefe 0..3 werden gelesen …
    for n in ["null", "eins", "zwei", "drei"] {
        assert!(names.contains(&n), "{n} fehlt: {names:?}");
    }
    // … das `Include` **in** der Datei auf Tiefe 3 nicht mehr.
    assert!(!names.contains(&"vier"), "Tiefe 4 gelesen: {names:?}");
    assert!(
        read.include_issues
            .iter()
            .any(|i| i.reason == SkipReason::IncludeTooDeep),
        "als nicht übernommen gemeldet: {:?}",
        read.include_issues
    );
}

#[test]
fn t_6_2_7_nicht_lesbare_include_datei_laesst_den_import_weiterlaufen() {
    let d = tempfile::tempdir().unwrap();
    let c = write(
        d.path(),
        "config",
        "Include gibtsnicht.conf\nHost a\n  HostName 1.1.1.1\n",
    );
    let read = read_ok(&c);
    // Der Import läuft weiter …
    let plan = plan_of(&read);
    assert!(plan.entries.iter().any(|e| e.name == "a"));
    // … und die Datei wird gemeldet.
    assert!(
        read.include_issues
            .iter()
            .any(|i| i.reason == SkipReason::IncludeNoMatch)
            || read
                .files
                .iter()
                .any(|f| f.status == FileStatus::Unreadable),
        "nicht gemeldet: {read:?}"
    );
}

#[test]
fn t_6_2_8_dateiliste_mit_vollem_pfad_in_lesereihenfolge() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "zweite.conf", "Host b\n");
    let c = write(d.path(), "config", "Host a\nInclude zweite.conf\n");
    let read = read_ok(&c);
    let read_files: Vec<&FileReport> = read
        .files
        .iter()
        .filter(|f| f.status == FileStatus::Read)
        .collect();
    assert_eq!(read_files.len(), 2);
    // Voller Pfad, nicht nur der Dateiname.
    for f in &read_files {
        assert!(
            Path::new(&f.path).is_absolute(),
            "nicht absolut: {}",
            f.path
        );
    }
    // Lesereihenfolge: einbindende Datei zuerst.
    assert!(read_files[0].path.ends_with("config"));
    assert!(read_files[1].path.ends_with("zweite.conf"));
    assert_eq!((read_files[0].depth, read_files[1].depth), (0, 1));
}

// ------------------------------------------------------- §6.4.5 Grenzen

#[test]
fn t_6_4_5a_gesamtgroesse_ueber_viele_dateien() {
    let d = tempfile::tempdir().unwrap();
    // Neun Dateien à 1 MiB: je Datei weit unter der Grenze, zusammen
    // darüber. Genau das ist der Punkt — die Grenze gilt über alle Dateien
    // zusammen (§9, Anmerkung zur Grenze).
    let filler = "# ".to_string() + &"x".repeat(1022);
    let one_mib: String = std::iter::repeat_n(filler.as_str(), 1024)
        .collect::<Vec<_>>()
        .join("\n");
    let mut inc = String::new();
    for i in 0..9 {
        let name = format!("big{i}.conf");
        write(d.path(), &name, &format!("Host big{i}\n{one_mib}\n"));
        inc.push_str(&format!("Include {name}\n"));
    }
    let c = write(d.path(), "config", &format!("{inc}Host a\n"));

    assert_eq!(expect_abort(&c), ImportAbort::TotalBytes);
}

#[test]
fn t_6_4_5b_dreitausend_host_bloecke_ueber_mehrere_dateien() {
    let d = tempfile::tempdir().unwrap();
    let mut inc = String::new();
    for f in 0..3 {
        let mut body = String::new();
        for i in 0..1_000 {
            body.push_str(&format!("Host h{f}_{i}\n"));
        }
        let name = format!("part{f}.conf");
        write(d.path(), &name, &body);
        inc.push_str(&format!("Include {name}\n"));
    }
    let c = write(d.path(), "config", &format!("{inc}Host a\n"));
    assert_eq!(expect_abort(&c), ImportAbort::HostBlocks);
}

#[test]
fn t_6_4_5b_grenze_knapp_darunter_geht_durch() {
    // Gegenprobe: ohne sie wäre „immer ablehnen" eine bestehende
    // Implementierung.
    let d = tempfile::tempdir().unwrap();
    let mut body = String::new();
    for i in 0..MAX_HOST_BLOCKS {
        body.push_str(&format!("Host h{i}\n"));
    }
    let c = write(d.path(), "config", &body);
    let read = read_ok(&c);
    assert_eq!(read.sources[0].parsed.blocks.len(), MAX_HOST_BLOCKS);
}

#[test]
fn t_6_4_5c_wert_mit_hunderttausend_zeichen() {
    let d = tempfile::tempdir().unwrap();
    let huge = "x".repeat(100_000);
    let c = write(d.path(), "config", &format!("Host a\n  HostName {huge}\n"));
    match read_import(&c) {
        Err(ImportAbort::ValueTooLong { line, file }) => {
            assert_eq!(line, 2);
            // Die Meldung trägt den Wert nicht (§3.1.5, §5.4).
            let msg = ImportAbort::ValueTooLong { line, file }.to_string();
            assert!(!msg.contains(&huge));
        }
        other => panic!("erwartet ValueTooLong, war {other:?}"),
    }
}

#[test]
fn t_6_4_5d_datei_die_sich_selbst_einbindet() {
    // Genau der Fall, an dem `ssh2-config` den Prozess mit einem
    // Stapelüberlauf beendete (§9/M-1). Hier: eine Meldung, kein Hänger.
    let d = tempfile::tempdir().unwrap();
    let c = write(d.path(), "selbst.conf", "Include selbst.conf\nHost a\n");
    let read = read_ok(&c);
    assert_eq!(
        read.files
            .iter()
            .filter(|f| f.status == FileStatus::Read)
            .count(),
        1,
        "genau einmal gelesen"
    );
    assert!(read
        .files
        .iter()
        .any(|f| f.status == FileStatus::AlreadyRead));
}

#[test]
fn t_6_4_5e_kette_a_nach_b_nach_a() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "b.conf", "Include a.conf\nHost b\n");
    let c = write(d.path(), "a.conf", "Include b.conf\nHost a\n");
    let read = read_ok(&c);
    assert_eq!(
        read.files
            .iter()
            .filter(|f| f.status == FileStatus::Read)
            .count(),
        2
    );
    assert!(read
        .files
        .iter()
        .any(|f| f.status == FileStatus::AlreadyRead));
    let plan = plan_of(&read);
    assert_eq!(plan.entries.len(), 2);
}

#[test]
fn t_6_4_5f_include_stern_im_eigenen_verzeichnis() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "nachbar.conf", "Host nachbar\n");
    // Das Muster trifft die einbindende Datei selbst mit.
    let c = write(d.path(), "config.conf", "Include *.conf\nHost a\n");
    let read = read_ok(&c);
    assert_eq!(
        read.files
            .iter()
            .filter(|f| f.status == FileStatus::Read && f.path.ends_with("config.conf"))
            .count(),
        1,
        "die gewählte Datei genau einmal"
    );
    let plan = plan_of(&read);
    // `a` genau einmal, nicht doppelt.
    assert_eq!(plan.entries.iter().filter(|e| e.name == "a").count(), 1);
    assert!(plan.entries.iter().any(|e| e.name == "nachbar"));
}

#[test]
fn t_3_1_13_gewaehlte_datei_nicht_lesbar() {
    let d = tempfile::tempdir().unwrap();
    let missing = d.path().join("gibtsnicht");
    assert_eq!(expect_abort(&missing), ImportAbort::ChosenUnreadable);
}

#[test]
fn t_3_1_13_kein_host_block() {
    let d = tempfile::tempdir().unwrap();
    let c = write(d.path(), "config", "Compression yes\nLogLevel DEBUG\n");
    assert_eq!(expect_abort(&c), ImportAbort::NoHostBlock);
}

#[test]
fn t_3_1_4a_gewaehlte_datei_ist_keine_ssh_config() {
    let d = tempfile::tempdir().unwrap();
    let c = write(d.path(), "config", "Liebe Kollegin,\nviele Gruesse\n");
    assert_eq!(expect_abort(&c), ImportAbort::ChosenNotSshConfig);
}

// ------------------------------------------- §6.4.9 Fremdinhalt

/// §6.4.9: Vier Fälle, je mit `Include` auf eine Datei, die keine
/// `ssh_config` ist. In **allen** gilt: Was auftaucht, ist genau zweierlei
/// — der volle **Pfad** der Datei und die Aussage „keine ssh_config,
/// übersprungen". Nie ein Byte ihres Inhalts.
#[test]
fn t_6_4_9_fremdinhalt_erscheint_nirgends() {
    const MARKER: &str = "ERKENNBARE_ZEICHENFOLGE_4711";
    let key = format!(
        "-----BEGIN OPENSSH PRIVATE KEY-----\n{MARKER}b3BlbnNzaA\n-----END OPENSSH PRIVATE KEY-----\n"
    );
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("a_key.txt", key.into_bytes()),
        (
            "b_text.txt",
            format!("Notiz\n{MARKER}\nEnde\n").into_bytes(),
        ),
        ("c_binary.bin", vec![0u8, 1, 2, b'X', 0, 255, b'Y']),
        (
            "d_badutf8.bin",
            vec![b'H', b'o', b's', b't', b' ', 0xFF, 0xFE, b'\n'],
        ),
    ];

    for (name, bytes) in cases {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join(name), &bytes).unwrap();
        let c = write(
            d.path(),
            "config",
            &format!("Include {name}\nHost a\n  HostName 1.1.1.1\n"),
        );
        let read = read_ok(&c);

        // Die Datei ist genannt — mit vollem Pfad — und als übersprungen
        // eingestuft.
        let rep = read
            .files
            .iter()
            .find(|f| f.path.ends_with(name))
            .unwrap_or_else(|| panic!("{name} nicht in der Dateiliste: {read:?}"));
        assert_eq!(
            rep.status,
            FileStatus::NotSshConfig,
            "{name} wurde nicht übersprungen"
        );

        // Und ihr Inhalt taucht nirgends auf — nicht im Plan, nicht in der
        // Dateiliste, nicht in einer Meldung.
        let plan = plan_of(&read);
        let dump = format!("{read:?}{plan:?}");
        assert!(
            !dump.contains(MARKER),
            "{name}: erkennbare Zeichenfolge ist entwichen"
        );
        for b in &bytes {
            // Jedes Nicht-ASCII-Byte der Binärdateien darf auch nicht als
            // Escape-Folge auftauchen.
            if *b > 127 {
                assert!(
                    !dump.contains(&format!("{b:#04x}")),
                    "{name}: Byte {b} ist entwichen"
                );
            }
        }
        // Der Import selbst läuft durch.
        assert!(plan.entries.iter().any(|e| e.name == "a"));
    }
}
