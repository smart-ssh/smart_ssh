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

// ------------------------------------------------------- §3.2.5 / §6.3.14

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
    let dir = tempfile::tempdir().unwrap();
    let other = dir.path().join("smart-ssh-export.conf");
    assert!(!super::resolves_to_user_ssh_config(&other));
}

#[test]
fn t_6_3_14_symlink_auf_ssh_verzeichnis_wird_erkannt() {
    let Some(home) = super::home_dir() else {
        return;
    };
    let real_ssh = home.join(".ssh");
    if !real_ssh.is_dir() {
        // Auf einer frischen Maschine ohne `~/.ssh` ist der Symlink-Fall
        // gar nicht herstellbar — der lexikalische Vergleich (oberer Test)
        // deckt den häufigsten Fall bereits ab.
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let link = dir.path().join("ssh-link");
    #[cfg(unix)]
    {
        if std::os::unix::fs::symlink(&real_ssh, &link).is_err() {
            return;
        }
        assert!(super::resolves_to_user_ssh_config(&link.join("config")));
    }
}
