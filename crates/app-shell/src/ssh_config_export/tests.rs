//! Tests zu Spec 0075, Schritt 4: die `~/.ssh/config`-Ablehnung (§3.2.5,
//! §6.3.14) und der äußere Zeuge `ssh -F … -G` (§6.3.2, §6.3.12, §6.3.13,
//! §9/Q-1 Punkt 5). Beides läuft hier und nicht in
//! `crates/core/src/profiles/ssh_config/export/tests.rs`, weil es entweder
//! ein Dateisystem (symlink-Auflösung) oder ein echtes `ssh`-Binary
//! braucht — beides hat `core` laut Architekturregel nicht.
//!
//! **Warum ein externer Zeuge nötig ist, nicht nur der Rundlauf über
//! unseren eigenen Parser:** Leser und Schreiber teilen dieselbe
//! Zitier-/Trennerroutine (`ssh_config::quoting`); ein symmetrisch falsches
//! Verständnis davon bestünde einen reinen Rundlauf-Test trotzdem (§9/Q-1
//! Punkt 5). `ssh -G` urteilt unabhängig von unserem Parser.

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use chrono::Utc;
use ssh_manager_core::profiles::ssh_config::build_export;
use ssh_manager_core::profiles::{AuthMethod, PostIngestPolicy, Server};
use ssh_manager_core::shared::ServerId;
use uuid::Uuid;

const LOCAL: ServerId = ServerId(Uuid::nil());

fn server(name: &str, host: &str, port: u16, user: &str) -> Server {
    Server {
        id: ServerId::new(),
        name: name.to_string(),
        host: host.to_string(),
        port,
        username: user.to_string(),
        group_id: None,
        tags: vec![],
        auth: AuthMethod::Agent,
        notes: String::new(),
        jump_host: None,
        post_ingest_policy: PostIngestPolicy::default(),
        ai_injection_check_enabled: false,
        sftp_server_path: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

/// `ssh -F <config> -G <host>` — prüft die Datei, baut **keine** Verbindung
/// (§6.3.2). Gibt die geparsten `schlüsselwort → wert`-Zeilen zurück; bei
/// Mehrfachnennung (z. B. `identityfile`) gewinnt die **erste** Zeile, wie
/// `ssh -G` sie auch ausgibt (Vorgaben stehen danach).
fn ssh_g(config: &Path, host: &str) -> (i32, HashMap<String, String>) {
    let output = Command::new("ssh")
        .arg("-F")
        .arg(config)
        .arg("-G")
        .arg(host)
        .output()
        .expect(
            "ssh-Binary nicht startbar — s. Moduldoc: dieser Test braucht einen echten ssh-Client",
        );

    let mut values = HashMap::new();
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let mut parts = line.splitn(2, ' ');
        if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
            values.entry(k.to_string()).or_insert_with(|| v.to_string());
        }
    }
    (output.status.code().unwrap_or(-1), values)
}

// -------------------------------------------------------------- §6.3.2

#[test]
fn t_6_3_2_ssh_g_akzeptiert_die_exportierte_datei() {
    let mut target = server("web1", "10.0.0.5", 2222, "max mustermann");
    target.auth = AuthMethod::IdentityFile {
        path: "/tmp/does-not-need-to-exist-for-ssh-dash-g".to_string(),
        passphrase_ref: None,
    };

    let plan = build_export(&[target], &[], LOCAL);

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("export.conf");
    std::fs::write(&file, &plan.text).unwrap();

    let (code, values) = ssh_g(&file, "web1");
    assert_eq!(code, 0, "ssh -G verwarf die exportierte Datei");

    // Auf den **konkreten Wert** prüfen, nicht nur aufs Vorkommen des
    // Schlüsselworts (§6.3.2, ausdrückliche Warnung der Spec: `ssh -G`
    // gibt `identityfile` auch aus seinen eigenen Vorgaben aus).
    assert_eq!(values.get("hostname").map(String::as_str), Some("10.0.0.5"));
    assert_eq!(values.get("port").map(String::as_str), Some("2222"));
    assert_eq!(
        values.get("user").map(String::as_str),
        Some("max mustermann"),
        "§9/Q-1 Punkt 5: der Wert mit Leerzeichen ist der äußere Zeuge für die Quoting-Regel"
    );
    assert_eq!(
        values.get("identityfile").map(String::as_str),
        Some("/tmp/does-not-need-to-exist-for-ssh-dash-g")
    );
}

// ------------------------------------------------------------- §6.3.12

#[test]
fn t_6_3_12_proxyjump_mit_umbenanntem_alias_besteht_ssh_g() {
    let bastion = server("prod bastion", "10.0.0.1", 22, "");
    let mut target = server("target", "10.0.0.2", 22, "");
    target.jump_host = Some(bastion.id);

    let plan = build_export(&[bastion, target], &[], LOCAL);
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("export.conf");
    std::fs::write(&file, &plan.text).unwrap();

    let alias = &plan
        .exported
        .iter()
        .find(|e| e.original_name == "target")
        .unwrap()
        .alias;
    let (code, values) = ssh_g(&file, alias);
    assert_eq!(code, 0);

    let bastion_alias = &plan
        .exported
        .iter()
        .find(|e| e.original_name == "prod bastion")
        .unwrap()
        .alias;
    assert_eq!(
        values.get("proxyjump").map(String::as_str),
        Some(bastion_alias.as_str())
    );
}

// -------------------------------------------------- §6.4.6a äußerer Zeuge

/// §6.4.6a verlangt ausdrücklich „`ssh -F … -G` besteht" — der Test in
/// `crates/core/src/profiles/ssh_config/export/tests.rs` prüft nur den
/// eigenen Alias-Rundlauf (spec-reviewer-Fund, Runde 1: das ist genau die
/// Symmetriefalle, die §9/Q-1 Punkt 5 ausschließen will). Hier der externe
/// Zeuge dazu, mit demselben bösartigen Namen wie dort.
#[test]
fn t_6_4_6a_ssh_g_besteht_mit_boesartigem_namen() {
    let s = server("evil\n#\"name", "10.0.0.1", 22, "");
    let plan = build_export(std::slice::from_ref(&s), &[], LOCAL);
    let alias = &plan.exported[0].alias;

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("export.conf");
    std::fs::write(&file, &plan.text).unwrap();

    let (code, values) = ssh_g(&file, alias);
    assert_eq!(code, 0, "ssh -G verwarf die Datei mit dem sanierten Alias");
    assert_eq!(values.get("hostname").map(String::as_str), Some("10.0.0.1"));
}

// -------------------------------------------- Fall 2/13 aus Review-Runde 1

/// §9/Q-1 Punkt 5: `ssh -G` als äußerer Zeuge entscheidet, welche
/// Quoting-Fassung stimmt — nicht der Rundlauf durch unseren eigenen Leser
/// (der `\"` nicht entschachtelt, s. `quoting.rs`-Moduldoc). Ein Wert mit
/// eingebettetem `"`.
#[test]
fn t_review1_quote_value_maskiert_eingebettetes_anfuehrungszeichen() {
    let s = server("web1", "10.0.0.1", 22, "a \"b c");
    let plan = build_export(std::slice::from_ref(&s), &[], LOCAL);

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("export.conf");
    std::fs::write(&file, &plan.text).unwrap();

    let (code, values) = ssh_g(&file, "web1");
    assert_eq!(code, 0, "ssh -G verwarf die Datei:\n{}", plan.text);
    assert_eq!(values.get("user").map(String::as_str), Some("a \"b c"));
}

/// spec-reviewer Fall 13, Runde 1: Ein Wert, der **unmittelbar vor dem
/// schließenden Anführungszeichen** auf `\` endet — ohne Maskierung des
/// Backslashs selbst frisst `\"` das schließende Zeichen, und `ssh` liest
/// den Rest der Zeile als Müll statt als eigenen Wert. *Gegenbeweis:* mit
/// `quote_value`s `\`-Maskierung entfernt, scheitert dieser Test (geprüft,
/// danach wiederhergestellt — s. Bericht: mit dem *ursprünglichen* Wert aus
/// Runde 1 [Leerzeichen vor dem `\`] bestand `ssh -G` überraschend auch
/// ohne Maskierung; erst dieser Wert — der `\` steht direkt vor dem
/// schließenden Zeichen — zeigt den Fehler zuverlässig).
#[test]
fn t_review1_quote_value_maskiert_trailing_backslash() {
    let s = server("web1", "10.0.0.1", 22, "a b\\");
    let plan = build_export(std::slice::from_ref(&s), &[], LOCAL);

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("export.conf");
    std::fs::write(&file, &plan.text).unwrap();

    let (code, values) = ssh_g(&file, "web1");
    assert_eq!(code, 0, "ssh -G verwarf die Datei:\n{}", plan.text);
    assert_eq!(values.get("user").map(String::as_str), Some("a b\\"));
}

// ------------------------------------------------------- §3.2.5 / §6.3.14
//
// Die meisten Fälle laufen über `resolves_to_ssh_config_under` mit einem
// `tempdir()` als injiziertem Home-Verzeichnis (spec-reviewer-Fund, Runde 1:
// die ursprüngliche Fassung testete nur gegen das *echte* `~` dieser
// Maschine — Fall 3/4 unten ließen sich damit gar nicht zuverlässig
// herstellen, weil `~/.ssh` nicht überall existiert). Der Wrapper
// `resolves_to_user_ssh_config` (echtes `~`) bekommt genau einen Test, der
// nur die Verdrahtung prüft.

#[test]
fn t_6_3_14_eigene_ssh_config_wird_als_ziel_abgelehnt() {
    let Some(home) = super::home_dir() else {
        return; // ohne Home-Verzeichnis in dieser Umgebung nicht prüfbar
    };
    let target = home.join(".ssh").join("config");
    assert!(super::resolves_to_user_ssh_config(&target));
}

#[test]
fn t_6_3_14_anderer_pfad_wird_nicht_abgelehnt() {
    let home = tempfile::tempdir().unwrap();
    let other_dir = tempfile::tempdir().unwrap();
    let other = other_dir.path().join("smart-ssh-export.conf");
    assert!(!super::resolves_to_ssh_config_under(&other, home.path()));
}

#[test]
fn t_6_3_14_symlink_auf_ssh_verzeichnis_wird_erkannt() {
    let home = tempfile::tempdir().unwrap();
    let real_ssh = home.path().join(".ssh");
    std::fs::create_dir_all(&real_ssh).unwrap();
    let target_dir = tempfile::tempdir().unwrap();
    let link = target_dir.path().join("ssh-link");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&real_ssh, &link).unwrap();
        assert!(super::resolves_to_ssh_config_under(
            &link.join("config"),
            home.path()
        ));
    }
}

/// spec-reviewer-Fund, Runde 1: `~/.ssh/Config`/`~/.ssh/CONFIG` wichen dem
/// lexikalischen **und** dem alten `file_name() ==`-Vergleich aus — auf
/// macOS (APFS/HFS+ Standardeinstellung) und Windows ist das aber dieselbe
/// Datei wie `~/.ssh/config`. *Gegenbeweis:* mit dem ursprünglichen
/// `path.file_name() == target.file_name()` (byteweise) statt
/// `filenames_match_case_insensitive` scheitert dieser Test (geprüft,
/// danach wiederhergestellt — s. Bericht).
#[test]
fn t_review1_andere_gross_kleinschreibung_wird_erkannt() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".ssh")).unwrap();
    for variant in ["Config", "CONFIG", "cOnFiG"] {
        let candidate = home.path().join(".ssh").join(variant);
        assert!(
            super::resolves_to_ssh_config_under(&candidate, home.path()),
            "{variant} wurde nicht als ~/.ssh/config erkannt"
        );
    }
    // Eine andere Datei im selben Ordner bleibt erlaubt — die
    // Groß-/Kleinschreibung darf nicht zu einer pauschalen Ablehnung jeder
    // Datei in `~/.ssh` führen.
    assert!(!super::resolves_to_ssh_config_under(
        &home.path().join(".ssh").join("known_hosts"),
        home.path()
    ));
}

/// spec-reviewer-Fund, Runde 1: Ein Symlink, der die gewählte Datei selbst
/// (nicht nur ihr Elternverzeichnis) auf die echte `~/.ssh/config` zeigen
/// lässt, wich der alten Prüfung aus — `canonicalize` lief nur über das
/// Elternverzeichnis von `path`. *Gegenbeweis:* ohne Fall 3 (Auflösen von
/// `path` selbst) scheitert dieser Test (geprüft, danach wiederhergestellt).
#[cfg(unix)]
#[test]
fn t_review1_symlink_auf_die_datei_selbst_wird_erkannt() {
    let home = tempfile::tempdir().unwrap();
    let ssh_dir = home.path().join(".ssh");
    std::fs::create_dir_all(&ssh_dir).unwrap();
    let real_config = ssh_dir.join("config");
    std::fs::write(&real_config, "# echte Konfiguration\n").unwrap();

    let other_dir = tempfile::tempdir().unwrap();
    let link = other_dir.path().join("smart-ssh-export.conf");
    std::os::unix::fs::symlink(&real_config, &link).unwrap();

    assert!(super::resolves_to_ssh_config_under(&link, home.path()));
}

/// spec-reviewer-Fund, Runde 2: `canonicalize` (Fall 3) löst **keine**
/// Hardlinks auf — zwei Hardlinks auf dieselbe Inode haben verschiedene
/// kanonische Pfade. *Gegenbeweis:* mit Fall 5 (Inode-Vergleich) entfernt,
/// scheitert dieser Test (geprüft, danach wiederhergestellt — s. Bericht).
#[cfg(unix)]
#[test]
fn t_review2_hardlink_auf_die_datei_selbst_wird_erkannt() {
    let home = tempfile::tempdir().unwrap();
    let ssh_dir = home.path().join(".ssh");
    std::fs::create_dir_all(&ssh_dir).unwrap();
    let real_config = ssh_dir.join("config");
    std::fs::write(&real_config, "# echte Konfiguration\n").unwrap();

    let other_dir = tempfile::tempdir().unwrap();
    let hardlink = other_dir.path().join("smart-ssh-export.conf");
    std::fs::hard_link(&real_config, &hardlink).unwrap();

    assert!(super::resolves_to_ssh_config_under(&hardlink, home.path()));
}
