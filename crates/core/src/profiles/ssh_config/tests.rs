//! Tests zu Spec 0075, Schritt 1 (§6.1, §6.4.3, §6.4.4, §6.4.4a, §6.4.6,
//! §6.4.7) und zu den Messbefunden aus §9/M-1.

use chrono::Utc;
use uuid::Uuid;

use super::parser::{parse_source, FileParse, SkippedKind};
use super::pattern::matches_pattern;
use super::plan::*;
use crate::filter::{Pattern, Rule, RuleAction, RuleId, RuleOrigin, Scope};
use crate::profiles::types::{AuthMethod, Group, GroupId, PostIngestPolicy, Server};
use crate::shared::ServerId;

// ---------------------------------------------------------------- Hilfen

fn parsed(text: &str) -> super::parser::ParsedFile {
    match parse_source(text.as_bytes()).expect("kein Grenzfehler") {
        FileParse::Parsed(p) => p,
        FileParse::NotSshConfig => panic!("als keine ssh_config eingestuft: {text:?}"),
    }
}

fn source(path: &str, text: &str) -> ImportSource {
    ImportSource {
        path: path.to_string(),
        parent: None,
        parsed: parsed(text),
    }
}

fn child(path: &str, parent: usize, text: &str) -> ImportSource {
    ImportSource {
        path: path.to_string(),
        parent: Some(parent),
        parsed: parsed(text),
    }
}

const LOCAL: ServerId = ServerId(Uuid::nil());

fn inv<'a>(servers: &'a [Server], groups: &'a [Group], rules: &'a [Rule]) -> Inventory<'a> {
    Inventory {
        servers,
        groups,
        rules,
        local_server_id: LOCAL,
    }
}

fn empty_plan(text: &str) -> ImportPlan {
    build_plan(&[source("/tmp/config", text)], inv(&[], &[], &[]))
}

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

fn entry<'a>(plan: &'a ImportPlan, name: &str) -> &'a PlannedEntry {
    plan.entries
        .iter()
        .find(|e| e.name == name)
        .unwrap_or_else(|| panic!("Eintrag {name} fehlt; da: {:?}", names(plan)))
}

fn names(plan: &ImportPlan) -> Vec<String> {
    plan.entries.iter().map(|e| e.name.clone()).collect()
}

/// Alle `ProxyJump`-Ablehnungen zu einem Eintrag.
fn pj_rejections(plan: &ImportPlan, name: &str) -> Vec<ProxyJumpRejection> {
    plan.skipped
        .iter()
        .filter(|s| s.entry.as_deref() == Some(name))
        .filter_map(|s| match s.reason {
            SkipReason::ProxyJump(r) => Some(r),
            _ => None,
        })
        .collect()
}

// ------------------------------------------------------- §6.1 Abbildung

#[test]
fn t_6_1_1_block_mit_hostname_port_user() {
    let plan = empty_plan("Host web1\n  HostName 10.0.0.1\n  Port 2222\n  User max\n");
    let e = entry(&plan, "web1");
    assert_eq!(e.host.value, "10.0.0.1");
    assert_eq!(e.port.value, 2222);
    assert_eq!(e.username.value, "max");
    // Herkunft steht an jedem Feld, das aus der Datei kam (§3.1.3).
    assert_eq!(e.host.origin.as_ref().unwrap().line, 2);
    assert_eq!(e.port.origin.as_ref().unwrap().line, 3);
    assert_eq!(e.username.origin.as_ref().unwrap().line, 4);
}

#[test]
fn t_6_1_2_ohne_hostname_gilt_der_alias() {
    let plan = empty_plan("Host web1.example.com\n  User max\n");
    let e = entry(&plan, "web1.example.com");
    assert_eq!(e.host.value, "web1.example.com");
    // Vorgabe, nicht aus der Datei.
    assert!(e.host.origin.is_none());
}

#[test]
fn t_6_1_3_ohne_port_gilt_22() {
    let plan = empty_plan("Host a\n  HostName 1.2.3.4\n");
    assert_eq!(entry(&plan, "a").port.value, 22);
    assert!(entry(&plan, "a").port.origin.is_none());
}

#[test]
fn t_6_1_4_direktiven_gross_klein_werte_unveraendert() {
    let plan = empty_plan("host a\n  HOSTNAME MixedCase.Example.COM\n  pOrT 2200\n  UsEr MaX\n");
    let e = entry(&plan, "a");
    // Direktivennamen ohne Rücksicht auf Groß-/Kleinschreibung erkannt …
    assert_eq!(e.port.value, 2200);
    // … Werte dagegen unverändert.
    assert_eq!(e.host.value, "MixedCase.Example.COM");
    assert_eq!(e.username.value, "MaX");
}

#[test]
fn t_6_1_5_anfuehrungszeichen_und_gleichheitszeichen() {
    let plan = empty_plan("Host a\n  Port=2222\n  User \"max mustermann\"\n  HostName = 1.2.3.4\n");
    let e = entry(&plan, "a");
    assert_eq!(e.port.value, 2222);
    assert_eq!(e.username.value, "max mustermann");
    assert_eq!(e.host.value, "1.2.3.4");
}

#[test]
fn t_6_1_6_mehrere_aliase_ergeben_mehrere_eintraege() {
    let plan = empty_plan("Host web1 web2\n  HostName 10.0.0.1\n  User max\n");
    assert_eq!(names(&plan), vec!["web1", "web2"]);
    for n in ["web1", "web2"] {
        assert_eq!(entry(&plan, n).host.value, "10.0.0.1");
        assert_eq!(entry(&plan, n).username.value, "max");
    }
}

#[test]
fn t_6_1_7_platzhalter_wird_vorgabe_und_schlagwort() {
    let plan = empty_plan("Host *.prod.de\n  User deploy\nHost web1.prod.de\n  HostName 1.2.3.4\n");
    let e = entry(&plan, "web1.prod.de");
    assert_eq!(e.username.value, "deploy");
    assert_eq!(
        e.tags.iter().map(|t| &t.tag).collect::<Vec<_>>(),
        vec!["*.prod.de"]
    );
    // §3.1.3: aus einem Platzhalterblock entsteht **kein** Profil.
    assert!(!names(&plan).contains(&"*.prod.de".to_string()));
    assert_eq!(names(&plan), vec!["web1.prod.de"]);
}

#[test]
fn t_6_1_8_der_erste_gewinnt() {
    let plan = empty_plan("Host web1.prod.de\n  User root\nHost *.prod.de\n  User deploy\n");
    // Der zuerst gelesene Wert bleibt stehen — OpenSSH-Regel.
    assert_eq!(entry(&plan, "web1.prod.de").username.value, "root");
}

#[test]
fn t_6_1_9_mehrere_muster_ergeben_mehrere_schlagworte() {
    let plan =
        empty_plan("Host *.prod.de\n  User deploy\nHost *.de\n  Port 2222\nHost web1.prod.de\n");
    let e = entry(&plan, "web1.prod.de");
    let tags: Vec<&String> = e.tags.iter().map(|t| &t.tag).collect();
    assert_eq!(tags, vec!["*.prod.de", "*.de"]);
    assert_eq!(e.username.value, "deploy");
    assert_eq!(e.port.value, 2222);
}

#[test]
fn t_6_1_10_host_stern_erzeugt_kein_schlagwort() {
    let plan = empty_plan("Host *\n  User deploy\nHost web1\n");
    let e = entry(&plan, "web1");
    assert_eq!(e.username.value, "deploy", "Vorgabe wirkt");
    assert!(e.tags.is_empty(), "aber `Host *` trägt keine Information");
}

#[test]
fn t_6_1_11_zweites_identityfile_wird_gemeldet() {
    let plan = empty_plan("Host a\n  IdentityFile /k/one\n  IdentityFile /k/two\n");
    let e = entry(&plan, "a");
    assert_eq!(e.identity_file.as_ref().unwrap().path, "/k/one");
    // Der zweite Wert steht als nicht übernommen da — mit Zeile, ohne Wert.
    let s = plan
        .skipped
        .iter()
        .find(|s| s.line == 3)
        .expect("zweite IdentityFile-Zeile gemeldet");
    assert_eq!(s.directive, SkippedKind::Directive("IdentityFile".into()));
}

#[test]
fn t_6_1_12_kommentare_und_leerzeilen_aendern_nichts() {
    let a = empty_plan("Host a\n  HostName 1.2.3.4\n  User max\n");
    let b = empty_plan(
        "# Kommentar\n\nHost a\n\n   # eingerückter Kommentar\n  HostName 1.2.3.4\n\n  User max\n",
    );
    assert_eq!(a.entries.len(), b.entries.len());
    let (ea, eb) = (entry(&a, "a"), entry(&b, "a"));
    assert_eq!(
        (ea.host.value.as_str(), ea.username.value.as_str()),
        ("1.2.3.4", "max")
    );
    assert_eq!(
        (eb.host.value.as_str(), eb.username.value.as_str()),
        ("1.2.3.4", "max")
    );
}

// ---------------------------------------- §4.2 / §9/Q-1 P.4: Match

#[test]
fn t_match_ist_blockgrenze_kein_fehlmerge() {
    // Der Messbefund aus §9/M-1, als Test: `ssh2-config` gab `Host a` hier
    // `user = "y"`. Wer den Fehlmerge nachbaut, lässt diesen Test scheitern.
    let plan = empty_plan("Host a\n  User x\nMatch host b\n  User y\n");
    assert_eq!(entry(&plan, "a").username.value, "x");
    // `Match` selbst und die Zeile darin stehen als nicht übernommen da.
    let reported: Vec<&SkippedReport> = plan.skipped.iter().collect();
    assert!(
        reported
            .iter()
            .any(|s| s.directive == SkippedKind::Directive("Match".into()) && s.line == 3),
        "Match gemeldet: {reported:?}"
    );
    assert!(
        reported
            .iter()
            .any(|s| s.directive == SkippedKind::Directive("User".into()) && s.line == 4),
        "User im Match-Block gemeldet: {reported:?}"
    );
}

#[test]
fn t_match_beendet_block_auch_fuer_folgende_hosts() {
    let plan = empty_plan("Host a\n  User x\nMatch host z\n  Port 9999\nHost b\n  User y\n");
    assert_eq!(
        entry(&plan, "a").port.value,
        22,
        "Port aus Match wirkt nicht"
    );
    assert_eq!(entry(&plan, "b").username.value, "y");
    assert_eq!(entry(&plan, "b").port.value, 22);
}

// ------------------------------------------ §3.1.5: Name, nie der Wert

#[test]
fn t_6_4_9a_direktivenname_ohne_wert() {
    let plan = empty_plan("Host a\n  Compression yes\n  UnknownDirective GEHEIMNISVERDAECHTIG\n");
    let names: Vec<String> = plan
        .skipped
        .iter()
        .filter_map(|s| match &s.directive {
            SkippedKind::Directive(d) => Some(d.clone()),
            SkippedKind::UnreadableLine => None,
        })
        .collect();
    assert!(names.contains(&"Compression".to_string()));
    assert!(names.contains(&"UnknownDirective".to_string()));
    // Zeilennummern sind da …
    assert!(plan.skipped.iter().any(|s| s.line == 2));
    assert!(plan.skipped.iter().any(|s| s.line == 3));
    // … und der Wert nirgends. Das ist der Kern von §5.4 Regel 2.
    let dump = format!("{plan:?}");
    assert!(
        !dump.contains("GEHEIMNISVERDAECHTIG"),
        "Wert einer nicht übernommenen Direktive ist im Plan gelandet"
    );
}

#[test]
fn t_3_1_5_unlesbare_zeile_meldet_nur_die_zeilennummer() {
    let plan = empty_plan("Host a\n  \u{1}\u{2}GEHEIM_MARKER wert\n");
    let s = plan
        .skipped
        .iter()
        .find(|s| s.line == 2)
        .expect("Zeile 2 gemeldet");
    assert_eq!(s.directive, SkippedKind::UnreadableLine);
    let dump = format!("{plan:?}");
    assert!(!dump.contains("GEHEIM_MARKER"), "Inhalt ist entwichen");
}

// ------------------------------------------------- §3.1.4a / §6.4.9

#[test]
fn t_3_1_4a_nul_byte_ist_keine_ssh_config() {
    let bytes = b"Host a\n\0\nHostName 1.2.3.4\n";
    assert_eq!(parse_source(bytes).unwrap(), FileParse::NotSshConfig);
}

#[test]
fn t_3_1_4a_ungueltiges_utf8_ist_keine_ssh_config() {
    let bytes = [b'H', b'o', b's', b't', b' ', 0xFF, 0xFE, b'\n'];
    assert_eq!(parse_source(&bytes).unwrap(), FileParse::NotSshConfig);
}

#[test]
fn t_3_1_4a_prosadatei_ist_keine_ssh_config() {
    // §6.4.9 (b): Eine Textdatei mit erkennbarer Zeichenfolge. Ihr erstes
    // Wort je Zeile ist formgültig — die Schlüsselwortliste ist deshalb
    // die Ebene, die hier greifen muss (§9/Q-1, Punkt 3).
    let text = "Liebe Kollegin\nGEHEIM_MARKER_42 steht hier\nViele Gruesse\n";
    assert_eq!(
        parse_source(text.as_bytes()).unwrap(),
        FileParse::NotSshConfig
    );
}

#[test]
fn t_3_1_4a_privater_schluessel_ist_keine_ssh_config() {
    // §6.4.9 (a)
    let key = "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAA\n-----END OPENSSH PRIVATE KEY-----\n";
    assert_eq!(
        parse_source(key.as_bytes()).unwrap(),
        FileParse::NotSshConfig
    );
}

#[test]
fn t_3_1_4a_echte_config_wird_erkannt() {
    // Gegenprobe: Ohne sie wäre „alles ablehnen" eine bestehende
    // Implementierung.
    assert!(matches!(
        parse_source(b"Host a\n  HostName 1.2.3.4\n").unwrap(),
        FileParse::Parsed(_)
    ));
}

// ------------------------------------------------- §6.4.7 Randzeichen

#[test]
fn t_6_4_7a_randzeichen_werden_entfernt() {
    let text = "Host a\n  HostName \u{FEFF}1.2.3.4\u{200B}\n";
    let plan = empty_plan(text);
    assert_eq!(entry(&plan, "a").host.value, "1.2.3.4");
}

#[test]
fn t_6_4_7b_zeichen_innerhalb_bleiben_erhalten() {
    let text = "Host a\n  HostName 1.2\u{200B}3.4\n";
    let plan = empty_plan(text);
    // Weicht bewusst vom bereinigten Wert ab — nur die Ränder werden
    // angefasst (§3.1.2).
    assert_eq!(entry(&plan, "a").host.value, "1.2\u{200B}3.4");
    assert_ne!(entry(&plan, "a").host.value, "1.23.4");
}

#[test]
fn t_6_4_7c_wert_nur_aus_randzeichen_gilt_als_nicht_angegeben() {
    let text = "Host a\n  HostName \u{FEFF}\u{200B}\n  User \u{2060}\n";
    let plan = empty_plan(text);
    let e = entry(&plan, "a");
    // „nicht angegeben" heißt: die Vorgaben aus §3.1.2 greifen.
    assert_eq!(e.host.value, "a");
    assert!(e.host.origin.is_none());
    assert_eq!(e.username.value, "");
    // Und es wird gemeldet, nicht verschluckt.
    assert!(plan.skipped.iter().any(|s| s.line == 2));
    assert!(plan.skipped.iter().any(|s| s.line == 3));
}

#[test]
fn t_3_1_2_trim_nach_anfuehrungszeichen() {
    let plan = empty_plan("Host a\n  User \" max\"\n");
    assert_eq!(entry(&plan, "a").username.value, "max");
}

// ------------------------------------------------- §6.4.6 Böse Aliase

#[test]
fn t_6_4_6_boesartige_aliase_bleiben_harmlose_namen() {
    let plan = empty_plan(
        "Host ../../etc/passwd\n  HostName 1.2.3.4\nHost $(whoami)\n  HostName 1.2.3.5\nHost `id`\n",
    );
    // Sie werden als Namen übernommen — aus einem Namen entsteht bei uns
    // weder ein Pfad noch ein Kommando.
    assert!(names(&plan).contains(&"../../etc/passwd".to_string()));
    assert!(names(&plan).contains(&"$(whoami)".to_string()));
    for e in &plan.entries {
        // Nirgends wird daraus ein Jump-Host oder ein Schlüsselpfad.
        assert!(e.jump.is_none());
        assert!(e.identity_file.is_none());
    }
}

#[test]
fn t_6_4_6_alias_mit_steuerzeichen_und_leerer_alias() {
    let plan = empty_plan("Host a\u{7}b\n  HostName 1.2.3.4\nHost \"\"\n  HostName 9.9.9.9\n");
    assert!(names(&plan).contains(&"a\u{7}b".to_string()));
    // §4.3: leerer Alias ⇒ kein Profil, sondern eine Meldung.
    assert!(!names(&plan).iter().any(|n| n.is_empty()));
    assert!(plan
        .skipped
        .iter()
        .any(|s| s.reason == SkipReason::EmptyAlias));
}

// ------------------------------------------------- §6.4.3 Sicherheit

#[test]
fn t_6_4_3_fremde_datei_setzt_keine_sicherheitseinstellung() {
    // Die Datei versucht es auf beiden Wegen: als Direktive und als
    // `# smart-ssh:`-Kommentar (§4.4 — Kommentare werden nie zurückgelesen).
    let plan = empty_plan(
        "Host a\n  HostName 1.2.3.4\n  PostIngestPolicy allow\n  AiInjectionCheck no\n\
         # smart-ssh: post_ingest_policy=standard\n# smart-ssh: ai_injection_check=false\n",
    );
    let e = entry(&plan, "a");
    // Der Plan trägt diese Felder gar nicht erst — es gibt keinen Weg, sie
    // aus der Datei zu setzen (§5.2). Was der Plan trägt, ist abschließend:
    let dump = format!("{e:?}");
    assert!(!dump.to_lowercase().contains("postingest"));
    assert!(!dump.to_lowercase().contains("injection"));
    // Beide Direktiven erscheinen als nicht übernommen.
    let names: Vec<String> = plan
        .skipped
        .iter()
        .filter_map(|s| match &s.directive {
            SkippedKind::Directive(d) => Some(d.clone()),
            _ => None,
        })
        .collect();
    assert!(names.contains(&"PostIngestPolicy".to_string()));
    assert!(names.contains(&"AiInjectionCheck".to_string()));
}

#[test]
fn t_6_4_3a_schlagwort_trifft_bestehende_filterregel() {
    let rules = vec![
        Rule {
            id: RuleId("r-allow".into()),
            pattern: Pattern::Glob("systemctl restart *".into()),
            action: RuleAction::Allow,
            scope: Scope::Tag("*.prod.de".into()),
            priority: 0,
            origin: RuleOrigin::User,
        },
        Rule {
            id: RuleId("r-deny".into()),
            pattern: Pattern::Glob("rm -rf *".into()),
            action: RuleAction::Deny,
            scope: Scope::Tag("*.prod.de".into()),
            priority: 0,
            origin: RuleOrigin::User,
        },
        Rule {
            id: RuleId("r-other".into()),
            pattern: Pattern::Glob("ls *".into()),
            action: RuleAction::Allow,
            scope: Scope::Tag("nicht-getroffen".into()),
            priority: 0,
            origin: RuleOrigin::User,
        },
    ];
    let plan = build_plan(
        &[source(
            "/tmp/config",
            "Host *.prod.de\n  User deploy\nHost web1.prod.de\n",
        )],
        inv(&[], &[], &rules),
    );
    let tag = &entry(&plan, "web1.prod.de").tags[0];
    assert_eq!(tag.tag, "*.prod.de");
    let hit: Vec<&RuleId> = tag.matched_rules.iter().map(|m| &m.rule_id).collect();
    assert_eq!(
        hit,
        vec![&RuleId("r-allow".into()), &RuleId("r-deny".into())]
    );
    // Die Regel, die auf ein anderes Schlagwort zeigt, wird nicht gemeldet.
    assert!(!tag
        .matched_rules
        .iter()
        .any(|m| m.rule_id == RuleId("r-other".into())));
}

#[test]
fn t_5_2a_schlagwort_ohne_regel_wird_nicht_gekennzeichnet() {
    // Gegenprobe zu 6.4.3a: ohne passende Regel bleibt die Liste leer —
    // sonst wäre „kennzeichnet alles" eine bestehende Implementierung.
    let plan = empty_plan("Host *.prod.de\n  User deploy\nHost web1.prod.de\n");
    assert!(entry(&plan, "web1.prod.de").tags[0]
        .matched_rules
        .is_empty());
}

// ------------------------------------------------- §3.1.8 Konflikte

#[test]
fn t_3_1_8_konflikt_ueber_namen_und_ueber_adresse() {
    let existing = vec![
        server("web1", "10.0.0.1", 22, "max"),
        server("anders-benannt", "10.0.0.9", 2222, "root"),
    ];
    let plan = build_plan(
        &[source(
            "/tmp/config",
            "Host web1\n  HostName 172.16.0.1\nHost neu\n  HostName 10.0.0.9\n  Port 2222\n  User root\nHost frei\n  HostName 10.0.0.77\n",
        )],
        inv(&existing, &[], &[]),
    );
    assert_eq!(
        entry(&plan, "web1").conflict.as_ref().unwrap().kind,
        ConflictKind::Name
    );
    assert_eq!(
        entry(&plan, "neu").conflict.as_ref().unwrap().kind,
        ConflictKind::Address
    );
    assert!(entry(&plan, "frei").conflict.is_none());
}

// ------------------------------------------- §6.4.4 / §6.4.4a ProxyJump

#[test]
fn t_3_1_6_einzelner_hop_auf_eintrag_im_import() {
    let plan = empty_plan("Host bastion\n  HostName 1.1.1.1\nHost x\n  ProxyJump bastion\n");
    let target = match entry(&plan, "x").jump {
        Some(JumpTarget::Planned(i)) => plan.entries[i].name.clone(),
        other => panic!("erwartet Planned, war {other:?}"),
    };
    assert_eq!(target, "bastion");
    // §3.1.11: das Ziel steht **vor** dem Profil, das auf es zeigt.
    let pos = |n: &str| plan.entries.iter().position(|e| e.name == n).unwrap();
    assert!(pos("bastion") < pos("x"));
}

#[test]
fn t_3_1_6_einzelner_hop_auf_bestand() {
    let existing = vec![server("bastion", "1.1.1.1", 22, "max")];
    let id = existing[0].id;
    let plan = build_plan(
        &[source("/tmp/config", "Host x\n  ProxyJump bastion\n")],
        inv(&existing, &[], &[]),
    );
    assert_eq!(entry(&plan, "x").jump, Some(JumpTarget::Existing(id)));
}

#[test]
fn t_3_1_6_proxyjump_none() {
    let plan = empty_plan("Host x\n  ProxyJump none\n");
    assert_eq!(entry(&plan, "x").jump, None);
    assert!(pj_rejections(&plan, "x").is_empty(), "none ist kein Fehler");
}

#[test]
fn t_3_1_6_hop_nicht_aufloesbar() {
    let plan = empty_plan("Host x\n  ProxyJump gibtsnicht\n");
    assert_eq!(entry(&plan, "x").jump, None);
    assert_eq!(
        pj_rejections(&plan, "x"),
        vec![ProxyJumpRejection::HopUnresolved]
    );
    // Das Profil selbst entsteht trotzdem (§3.1.6).
    assert_eq!(names(&plan), vec!["x"]);
}

#[test]
fn t_6_4_4a_a_mehr_hop_kette_richtige_richtung() {
    let plan = empty_plan("Host x\n  ProxyJump a,b,c\nHost a\nHost b\nHost c\n");
    let jump_name = |n: &str| match entry(&plan, n).jump {
        Some(JumpTarget::Planned(i)) => Some(plan.entries[i].name.clone()),
        Some(JumpTarget::Existing(_)) => panic!("unerwartet Bestand"),
        None => None,
    };
    // Genau diese Kanten, jede einzeln geprüft — eine seitenverkehrt
    // gebaute Kette wäre sonst grün (§6.4.4a a).
    assert_eq!(jump_name("x").as_deref(), Some("c"));
    assert_eq!(jump_name("c").as_deref(), Some("b"));
    assert_eq!(jump_name("b").as_deref(), Some("a"));
    assert_eq!(jump_name("a"), None);
    // §3.1.11: jedes Ziel vor seinem Nutzer.
    let pos = |n: &str| plan.entries.iter().position(|e| e.name == n).unwrap();
    assert!(pos("a") < pos("b"));
    assert!(pos("b") < pos("c"));
    assert!(pos("c") < pos("x"));
}

#[test]
fn t_6_4_4a_b_zwischenstation_im_bestand_kette_faellt_aus() {
    let existing = vec![server("b", "9.9.9.9", 22, "root")];
    let before = existing[0].clone();
    let plan = build_plan(
        &[source("/tmp/config", "Host x\n  ProxyJump a,b\nHost a\n")],
        inv(&existing, &[], &[]),
    );
    assert_eq!(entry(&plan, "x").jump, None);
    assert!(pj_rejections(&plan, "x").contains(&ProxyJumpRejection::IntermediateExists));
    // Das fremde Profil ist Feld für Feld unverändert — der Plan fasst es
    // nicht an (er kann es gar nicht).
    assert_eq!(existing[0], before);
    assert_eq!(names(&plan), vec!["x", "a"]);
}

#[test]
fn t_6_4_4a_c_zwei_ketten_verschiedene_vorgaenger() {
    let plan =
        empty_plan("Host x\n  ProxyJump a,b\nHost y\n  ProxyJump q,b\nHost a\nHost q\nHost b\n");
    assert_eq!(entry(&plan, "x").jump, None);
    assert_eq!(entry(&plan, "y").jump, None);
    assert_eq!(entry(&plan, "b").jump, None);
    assert!(pj_rejections(&plan, "x").contains(&ProxyJumpRejection::AmbiguousPredecessor));
    assert!(pj_rejections(&plan, "y").contains(&ProxyJumpRejection::AmbiguousPredecessor));
}

#[test]
fn t_6_4_4a_d_zwischenstation_mit_eigenem_proxyjump() {
    let plan = empty_plan("Host x\n  ProxyJump a,b\nHost b\n  ProxyJump q\nHost a\nHost q\n");
    // Zweite Quelle für `b.jump_host` ⇒ dieselbe Rechtsfolge.
    assert_eq!(entry(&plan, "x").jump, None);
    assert_eq!(entry(&plan, "b").jump, None);
    assert!(pj_rejections(&plan, "x").contains(&ProxyJumpRejection::OwnProxyJumpAndIntermediate));
    assert!(pj_rejections(&plan, "b").contains(&ProxyJumpRejection::OwnProxyJumpAndIntermediate));
}

#[test]
fn t_6_4_4_schleife_zwei_hosts() {
    let plan = empty_plan("Host a\n  ProxyJump b\nHost b\n  ProxyJump a\n");
    assert_eq!(entry(&plan, "a").jump, None);
    assert_eq!(entry(&plan, "b").jump, None);
    assert!(pj_rejections(&plan, "a").contains(&ProxyJumpRejection::Cycle));
    assert!(pj_rejections(&plan, "b").contains(&ProxyJumpRejection::Cycle));
    // Die Profile selbst entstehen normal (§3.1.6).
    assert_eq!(plan.entries.len(), 2);
}

#[test]
fn t_6_4_4_schleife_ueber_drei_hosts() {
    let plan = empty_plan("Host a\n  ProxyJump b\nHost b\n  ProxyJump c\nHost c\n  ProxyJump a\n");
    for n in ["a", "b", "c"] {
        assert_eq!(
            entry(&plan, n).jump,
            None,
            "{n} hat trotzdem einen Jump-Host"
        );
        assert!(pj_rejections(&plan, n).contains(&ProxyJumpRejection::Cycle));
    }
    assert_eq!(plan.entries.len(), 3);
}

#[test]
fn t_6_4_4_schleife_ueber_zwei_dateien() {
    let plan = build_plan(
        &[
            source("/tmp/config", "Host a\n  ProxyJump b\n"),
            child("/tmp/team.conf", 0, "Host b\n  ProxyJump a\n"),
        ],
        inv(&[], &[], &[]),
    );
    assert_eq!(entry(&plan, "a").jump, None);
    assert_eq!(entry(&plan, "b").jump, None);
    assert!(pj_rejections(&plan, "a").contains(&ProxyJumpRejection::Cycle));
}

#[test]
fn t_6_4_8_lokaler_pseudo_server_als_jump_host() {
    let mut local = server("lokal", "127.0.0.1", 22, "max");
    local.id = LOCAL;
    let existing = vec![local];
    let plan = build_plan(
        &[source("/tmp/config", "Host x\n  ProxyJump lokal\n")],
        inv(&existing, &[], &[]),
    );
    assert_eq!(entry(&plan, "x").jump, None);
    assert_eq!(
        pj_rejections(&plan, "x"),
        vec![ProxyJumpRejection::LocalPseudoServer]
    );
}

#[test]
fn t_3_1_6_hop_in_user_at_host_port_form() {
    let plan =
        empty_plan("Host bastion\n  HostName 1.1.1.1\nHost x\n  ProxyJump root@bastion:2222\n");
    assert!(matches!(
        entry(&plan, "x").jump,
        Some(JumpTarget::Planned(_))
    ));
}

// ------------------------------------------------- §3.1.9 IdentityFile

#[test]
fn t_3_1_9a_pfad_unveraendert_datei_bleibt_zu() {
    let plan = empty_plan("Host a\n  IdentityFile ~/.ssh/id_ed25519\n");
    let idf = entry(&plan, "a").identity_file.as_ref().unwrap();
    // Unverändert: kein realpath, kein Auflösen von `~` (§4.6, §5.5).
    assert_eq!(idf.path, "~/.ssh/id_ed25519");
    assert!(idf.usable_when_connecting);
}

#[test]
fn t_6_3_15_relativer_und_tilde_user_pfad_werden_markiert() {
    let plan = empty_plan(
        "Host rel\n  IdentityFile keys/id_rsa\nHost tu\n  IdentityFile ~max/.ssh/id_rsa\nHost abs\n  IdentityFile /home/max/.ssh/id_rsa\n",
    );
    assert!(
        !entry(&plan, "rel")
            .identity_file
            .as_ref()
            .unwrap()
            .usable_when_connecting
    );
    assert!(
        !entry(&plan, "tu")
            .identity_file
            .as_ref()
            .unwrap()
            .usable_when_connecting
    );
    assert!(
        entry(&plan, "abs")
            .identity_file
            .as_ref()
            .unwrap()
            .usable_when_connecting
    );
    // Trotzdem angelegt (§3.1.9 a).
    assert_eq!(names(&plan).len(), 3);
}

// ------------------------------------------------- §4.5 Gruppen

#[test]
fn t_4_5_gruppenbaum_folgt_den_dateien() {
    let plan = build_plan(
        &[
            source("/home/max/import/config", "Host a\n"),
            child("/home/max/import/team.conf", 0, "Host b\n"),
            child("/home/max/import/kunden.conf", 1, "Host c\n"),
        ],
        inv(&[], &[], &[]),
    );
    assert_eq!(plan.groups.len(), 3);
    assert_eq!(plan.groups[0].name, "config");
    assert_eq!(plan.groups[0].parent, None);
    assert_eq!(plan.groups[1].name, "team.conf");
    assert_eq!(plan.groups[1].parent, Some(0));
    assert_eq!(plan.groups[2].name, "kunden.conf");
    assert_eq!(plan.groups[2].parent, Some(1));
    // Jeder Server in der Gruppe seiner Herkunftsdatei.
    assert_eq!(entry(&plan, "a").group, 0);
    assert_eq!(entry(&plan, "b").group, 1);
    assert_eq!(entry(&plan, "c").group, 2);
}

#[test]
fn t_4_5_gruppenname_kollidiert_mit_bestand() {
    let groups = vec![Group {
        id: GroupId::new(),
        name: "config".into(),
        parent_id: None,
        notes: String::new(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }];
    let plan = build_plan(
        &[source("/home/max/import/config", "Host a\n")],
        inv(&[], &groups, &[]),
    );
    // Eine **neue** Gruppe mit angehängter Nummer, nicht in die bestehende
    // hineinschreiben (§4.5).
    assert_eq!(plan.groups[0].name, "config 2");
}

// ------------------------------------------------- §3.3 Grenze je Wert

#[test]
fn t_3_3_zu_langer_wert_bricht_ab() {
    let long = "x".repeat(super::parser::MAX_VALUE_CHARS + 1);
    let text = format!("Host a\n  HostName {long}\n");
    let err = parse_source(text.as_bytes()).expect_err("Grenze greift");
    assert_eq!(err.line, 2);
    // Die Meldung trägt die Zeile, nicht den Wert.
    assert!(!format!("{err:?}").contains(&long));
}

#[test]
fn t_3_3_wert_an_der_grenze_geht_noch_durch() {
    // Gegenprobe: ohne sie wäre „immer ablehnen" eine bestehende
    // Implementierung.
    let ok = "x".repeat(super::parser::MAX_VALUE_CHARS);
    let text = format!("Host a\n  HostName {ok}\n");
    assert!(matches!(
        parse_source(text.as_bytes()).unwrap(),
        FileParse::Parsed(_)
    ));
}

// ------------------------------------------------- Musterabgleich

#[test]
fn t_musterabgleich_nach_openssh_semantik() {
    assert!(matches_pattern("*.prod.de", "web1.prod.de"));
    assert!(matches_pattern("*", "irgendwas"));
    assert!(matches_pattern("web?", "web1"));
    assert!(!matches_pattern("web?", "web12"));
    assert!(!matches_pattern("*.prod.de", "web1.test.de"));
    // `*` trifft auch Punkte — anders als ein Dateiglob.
    assert!(matches_pattern("*.de", "a.b.c.de"));
    // §9/Q-1 Punkt 2: kein `globset`. `[` und `{` sind **wörtlich**.
    assert!(matches_pattern("web[1", "web[1"));
    assert!(!matches_pattern("web[1-9]", "web5"));
    assert!(matches_pattern("a{b,c}", "a{b,c}"));
}

#[test]
fn t_musterabgleich_bleibt_bei_bosartigem_muster_schnell() {
    // Ein rückverfolgender Abgleich bräuchte hier exponentiell lange.
    let pattern = "*a*a*a*a*a*a*a*a*a*a*a*a*b";
    let name = "a".repeat(64);
    let start = std::time::Instant::now();
    assert!(!matches_pattern(pattern, &name));
    assert!(
        start.elapsed() < std::time::Duration::from_secs(1),
        "Musterabgleich ist nicht linear"
    );
}

#[test]
fn t_negiertes_muster_schliesst_aus() {
    let plan = empty_plan(
        "Host *.prod.de !geheim.prod.de\n  User deploy\nHost web1.prod.de\nHost geheim.prod.de\n",
    );
    assert_eq!(entry(&plan, "web1.prod.de").username.value, "deploy");
    // Die Ausnahme greift: kein `deploy`, und auch kein Schlagwort.
    assert_eq!(entry(&plan, "geheim.prod.de").username.value, "");
    assert!(entry(&plan, "geheim.prod.de").tags.is_empty());
}

// ------------------------------------------------- Mehrere Dateien

#[test]
fn t_6_2_9_proxyjump_ueber_dateigrenze_aufgeloest() {
    let plan = build_plan(
        &[
            source("/tmp/config", "Host x\n  ProxyJump bastion\n"),
            child("/tmp/team.conf", 0, "Host bastion\n  HostName 1.1.1.1\n"),
        ],
        inv(&[], &[], &[]),
    );
    let t = match entry(&plan, "x").jump {
        Some(JumpTarget::Planned(i)) => plan.entries[i].name.clone(),
        other => panic!("erwartet Planned, war {other:?}"),
    };
    assert_eq!(t, "bastion");
}

#[test]
fn t_platzhalter_wirkt_ueber_dateigrenze() {
    let plan = build_plan(
        &[
            source("/tmp/config", "Host *.prod.de\n  User deploy\n"),
            child("/tmp/team.conf", 0, "Host web1.prod.de\n"),
        ],
        inv(&[], &[], &[]),
    );
    assert_eq!(entry(&plan, "web1.prod.de").username.value, "deploy");
}
