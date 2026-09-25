//! Tests zu Spec 0075, Schritt 4 (§6.3.1–3, §6.3.3a, §6.3.10–14, §6.4.2,
//! §6.4.6a) — soweit ohne echtes `ssh`-Binary und ohne Dateisystem prüfbar.
//! `ssh -F … -G` (§6.3.2) und die `~/.ssh/config`-Ablehnung (§3.2.5,
//! §6.3.14) sind app-shell-Integrationstests
//! (`crates/app-shell/src/ssh_config_export/tests.rs`) — dort steht auch,
//! warum.

use chrono::Utc;
use uuid::Uuid;

use super::*;
use crate::profiles::ssh_config::plan::{build_plan, ImportSource, Inventory};
use crate::profiles::ssh_config::{parse_source, FileParse};
use crate::profiles::types::{AuthMethod, CredentialRef, Group, GroupId, PostIngestPolicy, Server};
use crate::shared::ServerId;

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

/// Parst den exportierten Text wieder ein und baut daraus einen Plan gegen
/// einen leeren Bestand — die Grundlage jedes Rundlauf-Vergleichs (§6.3.3).
fn reparse(text: &str) -> crate::profiles::ssh_config::ImportPlan {
    let parsed = match parse_source(text.as_bytes()).expect("Export bleibt unter den Grenzen") {
        FileParse::Parsed(p) => p,
        FileParse::NotSshConfig => {
            panic!("eigener Export wird nicht als ssh_config erkannt:\n{text}")
        }
    };
    let source = ImportSource {
        path: "/tmp/export".to_string(),
        parent: None,
        parsed,
    };
    build_plan(
        &[source],
        Inventory {
            servers: &[],
            groups: &[],
            rules: &[],
            local_server_id: LOCAL,
        },
    )
}

// ------------------------------------------------------- §6.3.1 Grundform

// spec-reviewer-Fund, Runde 1: hieß zuvor `t_6_3_1_…`, prüfte aber die
// bedingten Felder (§3.2.1), nicht §6.3.1 (Vorschau==Ergebnis, Feld für
// Feld) — der echte §6.3.1-Test steht in
// `crates/app-shell/src/ssh_config_apply/tests.rs`
// (`t_6_3_1_vorschau_und_ergebnis_stimmen_ueberein`), wo Vorschau-DTO und
// angewandtes Ergebnis beide verfügbar sind.
#[test]
fn t_3_2_1_pflichtfelder_und_bedingte_felder() {
    let mut a = server("web1", "10.0.0.1", 22, "");
    a.id = ServerId::new();
    let mut b = server("web2", "10.0.0.2", 2222, "max");
    b.id = ServerId::new();

    let plan = build_export(&[a, b], &[], LOCAL);
    assert_eq!(plan.exported.len(), 2);

    assert!(plan.text.contains("Host web1\n"));
    assert!(plan.text.contains("HostName 10.0.0.1\n"));
    // Port 22 ⇒ **nicht** geschrieben (§3.2.1) — isoliert auf web1s Block,
    // sonst träfe die Prüfung fälschlich auf das Präfix von "Port 2222".
    let web1_block = plan.text.split("Host web1").nth(1).unwrap();
    let web1_block = web1_block.split("Host web2").next().unwrap();
    assert!(!web1_block.contains("Port"));
    // Leerer User ⇒ **nicht** geschrieben.
    assert!(!web1_block.contains("User"));

    assert!(plan.text.contains("Host web2\n"));
    assert!(plan.text.contains("Port 2222\n"));
    assert!(plan.text.contains("User max\n"));
}

// -------------------------------------------------- §6.3.3 Rundlauf (a)

#[test]
fn t_6_3_3_rundlauf_weg_a_identity_file() {
    let mut bastion = server("bastion", "10.0.0.1", 22, "root");
    bastion.id = ServerId::new();
    let mut target = server("target", "10.0.0.2", 2200, "deploy");
    target.id = ServerId::new();
    target.jump_host = Some(bastion.id);
    target.auth = AuthMethod::IdentityFile {
        path: "/home/max/.ssh/id_ed25519".to_string(),
        passphrase_ref: None,
    };

    let plan = build_export(&[bastion.clone(), target.clone()], &[], LOCAL);
    let reimported = reparse(&plan.text);

    let re_bastion = reimported
        .entries
        .iter()
        .find(|e| e.name == "bastion")
        .expect("bastion fehlt nach dem Rundlauf");
    let re_target = reimported
        .entries
        .iter()
        .find(|e| e.name == "target")
        .expect("target fehlt nach dem Rundlauf");

    assert_eq!(re_bastion.host.value, "10.0.0.1");
    assert_eq!(re_bastion.port.value, 22);
    assert_eq!(re_bastion.username.value, "root");

    assert_eq!(re_target.host.value, "10.0.0.2");
    assert_eq!(re_target.port.value, 2200);
    assert_eq!(re_target.username.value, "deploy");
    assert_eq!(
        re_target.identity_file.as_ref().map(|i| i.path.as_str()),
        Some("/home/max/.ssh/id_ed25519")
    );

    // Jump-Host-Kette: `target` zeigt (nach Namen) auf `bastion` — beide
    // sind in diesem Import neu, also eine echte Kante, kein Bestandsfall.
    let bastion_idx = reimported
        .entries
        .iter()
        .position(|e| e.name == "bastion")
        .unwrap();
    match re_target.jump {
        Some(crate::profiles::ssh_config::JumpTarget::Planned(t)) => assert_eq!(t, bastion_idx),
        other => panic!("erwartete geplante Jump-Kante, war {other:?}"),
    }
}

// ------------------------------------------ §6.3.3a Rundlauf (b) / Keychain

#[test]
fn t_6_3_3a_privatekey_erzeugt_kein_identityfile_und_keinen_schluessel() {
    let mut s = server("vault", "10.0.0.9", 22, "ops");
    s.auth = AuthMethod::PrivateKey {
        credential_ref: CredentialRef::new("server:vault:private_key"),
        passphrase_ref: None,
    };

    let plan = build_export(&[s], &[], LOCAL);

    assert!(!plan.text.contains("IdentityFile"));
    // §5.1 letzter Satz: kein Wert aus dem Schlüsselbund — nicht einmal der
    // (an sich unbedenkliche) Lookup-Key landet in der Datei.
    assert!(!plan.text.contains("server:vault:private_key"));
    assert!(plan.text.contains("# smart-ssh:"));
    assert!(plan.text.to_lowercase().contains("schlüsselbund"));
}

// -------------------------------------------------- §6.3.10 lokaler Server

#[test]
fn t_6_3_10_lokaler_pseudo_server_wird_nicht_exportiert() {
    let mut local = server("Lokales Terminal", "localhost", 22, "");
    local.id = LOCAL;
    let mut real = server("web1", "10.0.0.1", 22, "");
    real.id = ServerId::new();

    let plan = build_export(&[local, real], &[], LOCAL);

    assert_eq!(plan.exported.len(), 1);
    assert!(!plan.text.contains("Lokales Terminal"));
    assert!(!plan.text.contains("localhost"));
}

// --------------------------------------------- §6.3.11 ungültiger Alias

#[test]
fn t_6_3_11_name_mit_leerzeichen_und_raute_wird_gueltiger_alias() {
    let s = server("prod web #1", "10.0.0.1", 22, "");
    let plan = build_export(std::slice::from_ref(&s), &[], LOCAL);

    let exported = &plan.exported[0];
    assert_ne!(exported.alias, "prod web #1");
    assert!(exported.renamed);
    // Die neue Form besteht ausschließlich aus erlaubten Zeichen (§4.3).
    assert!(exported
        .alias
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-'));
    assert!(plan.text.contains(&format!("Host {}\n", exported.alias)));
}

// ------------------------------------------- §6.3.12 Alias-Kollision

#[test]
fn t_6_3_12_kollidierende_aliase_und_umbenannter_jump_host() {
    // Beide bilden nach `4.3` auf denselben Rumpf ab.
    let a = server("prod web", "10.0.0.1", 22, "");
    let b = server("prod-web", "10.0.0.2", 22, "");
    let mut target = server("target", "10.0.0.3", 22, "");
    target.jump_host = Some(b.id);

    let plan = build_export(&[a.clone(), b.clone(), target], &[], LOCAL);

    let alias_a = &plan.exported.iter().find(|e| e.id == a.id).unwrap().alias;
    let alias_b = &plan.exported.iter().find(|e| e.id == b.id).unwrap().alias;
    assert_ne!(
        alias_a, alias_b,
        "unterschiedliche Server müssen unterschiedliche Aliase bekommen"
    );

    // `target`s ProxyJump nennt den tatsächlich geschriebenen Alias von `b`.
    assert!(plan.text.contains(&format!("ProxyJump {alias_b}\n")));
}

// ----------------------------------------------- §6.3.13 Wert mit Leerzeichen

#[test]
fn t_6_3_13_username_mit_leerzeichen_wird_gequotet() {
    let s = server("web1", "10.0.0.1", 22, "max mustermann");
    let plan = build_export(&[s], &[], LOCAL);

    assert!(plan.text.contains("User \"max mustermann\"\n"));

    // Und der Rundlauf durch unseren eigenen Leser liefert den vollen Wert
    // zurück — genau der Fall aus §9/Q-1 Punkt 5, der den symmetrischen
    // Fehler ausschließt, den ein reiner Rundlauf sonst verdecken könnte.
    let reimported = reparse(&plan.text);
    let e = reimported.entries.first().expect("ein Eintrag");
    assert_eq!(e.username.value, "max mustermann");
}

// --------------------------------------------------------- §6.4.2 Leak-Test

#[test]
fn t_6_4_2_kein_geheimnis_im_export() {
    let mut pw = server("pw-server", "10.0.0.5", 22, "root");
    pw.auth = AuthMethod::Password {
        credential_ref: CredentialRef::new("server:pw-server:password"),
    };
    let mut key = server("key-server", "10.0.0.6", 22, "root");
    key.auth = AuthMethod::PrivateKey {
        credential_ref: CredentialRef::new("server:key-server:private_key"),
        passphrase_ref: Some(CredentialRef::new("server:key-server:passphrase")),
    };
    // Gepflanzte, eindeutig erkennbare "Geheimnisse" — tauchten sie im
    // Export auf, wäre das der Fund.
    const PLANTED_SECRET_MARKERS: &[&str] = &[
        "server:pw-server:password",
        "server:key-server:private_key",
        "server:key-server:passphrase",
        "-----BEGIN OPENSSH PRIVATE KEY-----",
    ];

    let plan = build_export(&[pw, key], &[], LOCAL);

    for marker in PLANTED_SECRET_MARKERS {
        assert!(
            !plan.text.contains(marker),
            "Geheimnis-Marker {marker:?} tauchte im Export auf:\n{}",
            plan.text
        );
    }
    // Keine der beiden Anmeldearten schreibt eine `IdentityFile`- oder
    // sonstige Geheimnis-tragende Zeile.
    assert!(!plan.text.contains("IdentityFile"));
    assert!(!plan.text.contains("Password"));
}

// ----------------------------------------------- §6.4.6a bösartige Namen

#[test]
fn t_6_4_6a_name_mit_newline_raute_anfuehrungszeichen() {
    let s = server("evil\n#\"name", "10.0.0.1", 22, "");
    let plan = build_export(std::slice::from_ref(&s), &[], LOCAL);

    let exported = &plan.exported[0];
    // Kein Zeilenumbruch, keine Raute, kein Anführungszeichen im Alias —
    // sonst entstünde eine zweite Zeile oder ein abgebrochener Kommentar.
    assert!(!exported.alias.contains('\n'));
    assert!(!exported.alias.contains('#'));
    assert!(!exported.alias.contains('"'));

    // Die Datei enthält also auch nur die erwartete Anzahl `Host`-Zeilen —
    // kein injizierter zweiter Block.
    assert_eq!(plan.text.matches("Host ").count(), 1);

    // Und der Rundlauf liefert genau diesen einen Eintrag zurück.
    let reimported = reparse(&plan.text);
    assert_eq!(reimported.entries.len(), 1);
    assert_eq!(reimported.entries[0].name, exported.alias);
}

/// spec-reviewer Fall 3, Runde 1: `username` hat beim Anlegen von Hand
/// keinen Format-Zwang (§1.3) — ein eingebettetes `\n` durfte nicht dazu
/// führen, dass aus dem Rest des Werts eine eigene, wirksame Zeile wird.
#[test]
fn t_review1_username_mit_newline_bricht_nicht_aus_dem_wert_aus() {
    let s = server("web1", "10.0.0.1", 22, "max\nProxyJump evil.example.com");
    let plan = build_export(std::slice::from_ref(&s), &[], LOCAL);

    assert!(
        !plan.text.contains("\nProxyJump evil.example.com"),
        "der Username brach aus der Zeile aus:\n{}",
        plan.text
    );
}

// ----------------------------------------------------- Weitere Kernfälle

#[test]
fn leerer_name_ergibt_server_n() {
    let s = server("***", "10.0.0.1", 22, "");
    let plan = build_export(&[s], &[], LOCAL);
    assert!(plan.exported[0].alias.starts_with("server-"));
}

#[test]
fn gruppenname_erscheint_als_kommentar_nicht_als_direktive() {
    let group_id = GroupId::new();
    let group = Group {
        id: group_id,
        name: "Produktion".to_string(),
        parent_id: None,
        notes: String::new(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    let mut s = server("web1", "10.0.0.1", 22, "");
    s.group_id = Some(group_id);

    let plan = build_export(&[s], std::slice::from_ref(&group), LOCAL);
    assert!(plan.text.contains("# smart-ssh: Gruppe „Produktion“"));
}

// ------------------------- spec-reviewer-Fund, Runde 1: Kommentar-Ausbruch

/// Ein Gruppenname ist der Name der Datei, aus der die Gruppe beim Import
/// entstand (§4.5) — roh übernommen, nicht saniert wie ein `Host`-Alias
/// (§4.3 gilt nur für Aliase). Ein `\n` darin beendete die `#`-Kommentar-
/// zeile vorzeitig; alles danach stand als **eigene, wirksame** Zeile *vor*
/// dem ersten `Host`-Block und galt damit („der erste gewinnt") für jeden
/// Host der Datei. *Gegenbeweis:* mit `sanitize_comment_text` durch die
/// Identität ersetzt, scheitert dieser Test (geprüft, danach
/// wiederhergestellt — s. Bericht).
#[test]
fn t_review1_gruppenname_mit_newline_bricht_nicht_aus_dem_kommentar_aus() {
    let group_id = GroupId::new();
    let group = Group {
        id: group_id,
        name: "x\nProxyJump evil.example.com\n#.conf".to_string(),
        parent_id: None,
        notes: String::new(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    let mut s = server("web1", "10.0.0.1", 22, "");
    s.group_id = Some(group_id);

    let plan = build_export(&[s], std::slice::from_ref(&group), LOCAL);

    assert!(
        !plan.text.contains("\nProxyJump evil.example.com\n"),
        "der Gruppenname brach aus dem Kommentar aus:\n{}",
        plan.text
    );
    // Jede Zeile ist entweder ein Kommentar oder Teil des einen Blocks —
    // keine zusätzliche, nicht eingerückte `ProxyJump`-Zeile.
    for line in plan.text.lines() {
        assert!(
            line.starts_with('#')
                || line.is_empty()
                || line.starts_with("Host")
                || line.starts_with("    "),
            "unerwartete Zeile außerhalb eines Kommentars/Blocks: {line:?}"
        );
    }
}

/// Dasselbe Schutzziel für Schlagworte (§6.4.6, Exportseite) — ein
/// Schlagwort kann wörtlich aus einer fremden `ssh_config` stammen
/// (§3.1.3).
#[test]
fn t_review1_schlagwort_mit_newline_bricht_nicht_aus_dem_kommentar_aus() {
    let mut s = server("web1", "10.0.0.1", 22, "");
    s.tags = vec!["x\nUser root\n#".to_string()];

    let plan = build_export(&[s], &[], LOCAL);

    assert!(
        !plan.text.contains("\nUser root\n"),
        "das Schlagwort brach aus dem Kommentar aus:\n{}",
        plan.text
    );
}

/// spec-reviewer-Fund, Runde 2 (Fall 4 der Adversarial-Tabelle): dieselbe
/// dritte Kommentar-Senke — `sftp_server_path` — war im Code schon
/// saniert, aber ungeprüft.
#[test]
fn t_review2_sftp_server_path_mit_newline_bricht_nicht_aus_dem_kommentar_aus() {
    let mut s = server("web1", "10.0.0.1", 22, "");
    s.sftp_server_path = Some("/usr/lib/sftp\nProxyJump evil.example.com\n#".to_string());

    let plan = build_export(&[s], &[], LOCAL);

    assert!(
        !plan.text.contains("\nProxyJump evil.example.com\n"),
        "der sftp_server_path brach aus dem Kommentar aus:\n{}",
        plan.text
    );
}

/// §1.3: das Anlegen von Hand kennt keinen Pflichtfeld-Check — ein leerer
/// `host` ist erreichbar. `HostName ""` ist gegenüber echtem `ssh`
/// fragwürdig; die Zeile bleibt deshalb ganz weg (§3.2.1 setzt implizit
/// einen nicht-leeren Wert voraus, s. Moduldoc).
#[test]
fn leerer_host_erzeugt_keine_hostname_zeile() {
    let s = server("web1", "", 22, "");
    let plan = build_export(&[s], &[], LOCAL);
    assert!(!plan.text.contains("HostName"));
}
