//! Tests zu Spec 0075, Schritt 3 (§6.3.4–9, §6.3.15–16, §6.4.1, §6.4.1a,
//! §6.4.1b, §6.4.3a, §6.4.8).

use std::sync::Mutex;

use ssh_manager_core::profiles::ssh_config::{build_plan, ImportSource, Inventory};
use ssh_manager_core::profiles::{AuthMethod, ProfileStore};
use uuid::Uuid;

use super::*;
use crate::test_support::{InMemoryCredentialStore, InMemoryProfileStore};

const MARKER_KEY: &str =
    "-----BEGIN OPENSSH PRIVATE KEY-----\nGEHEIMER_SCHLUESSEL_4711\n-----END OPENSSH PRIVATE KEY-----";

/// Zählt mit, welche Pfade geöffnet wurden — das ist der Zeuge für §5.1 und
/// §6.4.1/1a/1b. Ohne ihn wäre „hat nichts geöffnet" nicht prüfbar.
#[derive(Default)]
struct SpyKeyFiles {
    opened: Mutex<Vec<String>>,
    /// Pfade, die absichtlich scheitern sollen.
    fail: Vec<(String, IdentityFallbackReason)>,
}

impl KeyFileSource for SpyKeyFiles {
    fn read_key(&self, path: &str) -> Result<String, IdentityFallbackReason> {
        self.opened.lock().unwrap().push(path.to_string());
        if let Some((_, why)) = self.fail.iter().find(|(p, _)| p == path) {
            return Err(*why);
        }
        Ok(MARKER_KEY.to_string())
    }
}

fn keychain() -> credentials_keyring::KeychainAvailability {
    credentials_keyring::KeychainAvailability::Available
}

fn plan_from(text: &str) -> ssh_manager_core::profiles::ssh_config::ImportPlan {
    plan_from_inv(text, &[], &[])
}

fn plan_from_inv(
    text: &str,
    servers: &[ssh_manager_core::profiles::Server],
    rules: &[ssh_manager_core::filter::Rule],
) -> ssh_manager_core::profiles::ssh_config::ImportPlan {
    let parsed = match ssh_manager_core::profiles::ssh_config::parse_source(text.as_bytes())
        .expect("kein Grenzfehler")
    {
        ssh_manager_core::profiles::ssh_config::FileParse::Parsed(p) => p,
        other => panic!("erwartet Parsed, war {other:?}"),
    };
    build_plan(
        &[ImportSource {
            path: "/tmp/config".to_string(),
            parent: None,
            parsed,
        }],
        Inventory {
            servers,
            groups: &[],
            rules,
            local_server_id: ssh_manager_core::shared::ServerId(Uuid::nil()),
        },
    )
}

struct Fixture {
    store: InMemoryProfileStore,
    creds: InMemoryCredentialStore,
}

impl Fixture {
    fn new() -> Self {
        Self {
            store: InMemoryProfileStore::new(),
            creds: InMemoryCredentialStore::new(),
        }
    }

    async fn apply(
        &self,
        plan: &ssh_manager_core::profiles::ssh_config::ImportPlan,
        choices: &[EntryChoice],
        keys: &dyn KeyFileSource,
    ) -> CommandResult<ApplyOutcome> {
        apply_import(plan, choices, &self.store, &self.creds, keychain(), keys).await
    }

    async fn servers(&self) -> Vec<ssh_manager_core::profiles::Server> {
        self.store.list_servers().await.expect("list")
    }

    async fn server(&self, name: &str) -> ssh_manager_core::profiles::Server {
        self.servers()
            .await
            .into_iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("Server {name} fehlt"))
    }
}

fn choose(index: usize, mode: IdentityMode) -> EntryChoice {
    EntryChoice {
        index,
        selected: true,
        identity_mode: mode,
        rename_to: None,
        dropped_tags: Vec::new(),
    }
}

// --------------------------------------------- §6.4.1: Weg (a) liest nichts

#[tokio::test]
async fn t_6_4_1_weg_a_liest_keine_schluesseldatei() {
    let plan = plan_from("Host a\n  HostName 1.1.1.1\n  IdentityFile /keys/id_ed25519\n");
    let f = Fixture::new();
    let spy = SpyKeyFiles::default();
    // Vorgabeeinstellung = Weg (a): keine Wahl übergeben.
    let out = f.apply(&plan, &[], &spy).await.expect("Import");

    assert_eq!(out.created_servers, 1);
    // **Keine** Datei wurde geöffnet. Der Test scheitert, sobald jemand
    // „hilfsbereit" den Schlüssel einliest.
    assert!(
        spy.opened.lock().unwrap().is_empty(),
        "geöffnet: {:?}",
        spy.opened.lock().unwrap()
    );
    // Der Pfad steht in der Anmeldeart, unverändert (§4.6).
    let s = f.server("a").await;
    match &s.auth {
        AuthMethod::IdentityFile {
            path,
            passphrase_ref,
        } => {
            assert_eq!(path, "/keys/id_ed25519");
            assert!(passphrase_ref.is_none());
        }
        other => panic!("erwartet IdentityFile, war {other:?}"),
    }
    // Und der Inhalt kommt nirgends vor — nicht im Plan, nicht im Profil.
    let dump = format!("{plan:?}{:?}", f.servers().await);
    assert!(!dump.contains("GEHEIMER_SCHLUESSEL"));
}

/// §6.4.1b — **Die Vorschau öffnet nichts.** Der Nachweis läuft über den
/// echten Vorschau-Pfad (`read_import` + `build_plan`) gegen echte Dateien
/// auf der Platte, und die Schlüsseldateien sind so präpariert, dass jedes
/// Öffnen auffiele: Rechte `000`.
///
/// Die erste Fassung dieses Tests konnte nicht scheitern (Review-Runde 1):
/// Sie rief die Vorschau nie auf, sondern prüfte nur, dass eine frische
/// Fixture leer ist.
#[cfg(unix)]
#[tokio::test]
async fn t_6_4_1b_die_vorschau_oeffnet_nichts() {
    use std::os::unix::fs::PermissionsExt;

    let d = tempfile::tempdir().unwrap();
    // Drei Schlüsseldateien mit erkennbarem Inhalt, für niemanden lesbar.
    let mut key_paths = Vec::new();
    for n in ["a", "b", "c"] {
        let p = d.path().join(format!("id_{n}"));
        std::fs::write(&p, MARKER_KEY).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o000)).unwrap();
        key_paths.push(p);
    }
    let cfg = d.path().join("config");
    let mut text = String::new();
    for (n, p) in ["a", "b", "c"].iter().zip(&key_paths) {
        text.push_str(&format!("Host {n}\n  IdentityFile {}\n", p.display()));
    }
    std::fs::write(&cfg, &text).unwrap();

    // Der **echte** Vorschau-Pfad.
    let read = crate::ssh_config_import::read_import(&cfg).expect("Vorschau");
    let plan = build_plan(
        &read.sources,
        Inventory {
            servers: &[],
            groups: &[],
            rules: &[],
            local_server_id: ssh_manager_core::shared::ServerId(Uuid::nil()),
        },
    );

    // Die Pfade stehen im Plan — der Inhalt nirgends. Wäre eine der Dateien
    // geöffnet worden, wäre der Marker hier (oder der Lauf wäre an den
    // Rechten gescheitert).
    assert_eq!(plan.entries.len(), 3);
    for e in &plan.entries {
        assert!(e.identity_file.is_some(), "{} ohne Pfad", e.name);
    }
    let dump = format!("{read:?}{plan:?}");
    assert!(
        !dump.contains("GEHEIMER_SCHLUESSEL"),
        "die Vorschau hat eine Schlüsseldatei geöffnet"
    );

    // Und der Nutzer bricht ab: nichts angewandt, nichts angelegt (§6.3.8).
    let f = Fixture::new();
    assert!(f.servers().await.is_empty());
    assert!(f.creds.secrets.lock().unwrap().is_empty());
}

#[tokio::test]
async fn t_6_4_1a_weg_b_liest_genau_die_angekuendigten_dateien() {
    let plan = plan_from(
        "Host a\n  IdentityFile /k/a\nHost b\n  IdentityFile /k/b\nHost c\n  IdentityFile /k/c\n",
    );
    let f = Fixture::new();
    let spy = SpyKeyFiles::default();
    // a und c auf Weg (b), b **abgewählt**.
    let choices = vec![
        choose(0, IdentityMode::IntoKeychain),
        EntryChoice {
            index: 1,
            selected: false,
            identity_mode: IdentityMode::IntoKeychain,
            rename_to: None,
            dropped_tags: Vec::new(),
        },
        choose(2, IdentityMode::IntoKeychain),
    ];
    let out = f.apply(&plan, &choices, &spy).await.expect("Import");
    assert_eq!(out.created_servers, 2);

    // Genau die zwei gewählten Dateien — und die des abgewählten **nicht**.
    let opened = spy.opened.lock().unwrap().clone();
    assert_eq!(opened, vec!["/k/a".to_string(), "/k/c".to_string()]);
    assert!(!opened.contains(&"/k/b".to_string()));

    // Die zwei liegen als Schlüsselbund-Schlüssel vor …
    for n in ["a", "c"] {
        match &f.server(n).await.auth {
            AuthMethod::PrivateKey { passphrase_ref, .. } => {
                // §3.1.9 (b): keine Passphrase beim Import erfragt.
                assert!(passphrase_ref.is_none(), "{n} hat eine passphrase_ref");
            }
            other => panic!("{n}: erwartet PrivateKey, war {other:?}"),
        }
    }
    // … und der Inhalt taucht weder im Plan noch in den Profilen auf (§5.1).
    let dump = format!("{plan:?}{:?}", f.servers().await);
    assert!(
        !dump.contains("GEHEIMER_SCHLUESSEL"),
        "Schlüsselinhalt ist entwichen"
    );

    // §3.1.9 (b) / Spec 0076 C-4: Was gelesen wurde, liegt **byte-gleich**
    // im Schlüsselbund — auch ein verschlüsselter Schlüssel wird nicht
    // umgeformt. Diese Zusicherung fiel beim Umstellen auf den echten
    // `KeyFileReader` aus den Tests heraus (Lockerungs-Gegencheck, Runde 2);
    // sie gehört hierher, weil sie die Durchleitung in `apply_import`
    // betrifft, nicht die Treue des Lesers selbst (das ist Spec 0076).
    let secrets = f.creds.secrets.lock().unwrap();
    let stored: Vec<&secrecy::SecretString> = secrets
        .iter()
        .filter(|(k, _)| k.ends_with(":private_key"))
        .map(|(_, v)| v)
        .collect();
    assert_eq!(stored.len(), 2, "zwei Schlüssel erwartet");
    for v in stored {
        assert_eq!(
            v.expose_secret(),
            MARKER_KEY,
            "Inhalt wurde beim Ablegen verändert"
        );
    }
}

#[tokio::test]
async fn t_6_4_1a_fehlende_unlesbare_und_ungueltige_datei_fallen_auf_weg_a() {
    let plan = plan_from(
        "Host fehlt\n  IdentityFile /k/fehlt\nHost unlesbar\n  IdentityFile /k/unlesbar\nHost keinkey\n  IdentityFile /k/keinkey\nHost gut\n  IdentityFile /k/gut\n",
    );
    let f = Fixture::new();
    let spy = SpyKeyFiles {
        opened: Default::default(),
        fail: vec![
            ("/k/fehlt".into(), IdentityFallbackReason::Missing),
            ("/k/unlesbar".into(), IdentityFallbackReason::Unreadable),
            ("/k/keinkey".into(), IdentityFallbackReason::NotAKey),
        ],
    };
    let choices: Vec<EntryChoice> = (0..4)
        .map(|i| choose(i, IdentityMode::IntoKeychain))
        .collect();
    let out = f.apply(&plan, &choices, &spy).await.expect("Import");

    // Der Import scheitert **nicht** — alle vier entstehen.
    assert_eq!(out.created_servers, 4);
    // Die drei fallen auf Weg (a) zurück, mit Grund.
    assert_eq!(out.identity_fallbacks.len(), 3);
    let reason = |n: &str| {
        out.identity_fallbacks
            .iter()
            .find(|fb| fb.entry == n)
            .map(|fb| fb.reason)
    };
    assert_eq!(reason("fehlt"), Some(IdentityFallbackReason::Missing));
    assert_eq!(reason("unlesbar"), Some(IdentityFallbackReason::Unreadable));
    assert_eq!(reason("keinkey"), Some(IdentityFallbackReason::NotAKey));
    // Sie tragen den Pfad in der Anmeldeart …
    for n in ["fehlt", "unlesbar", "keinkey"] {
        assert!(
            matches!(f.server(n).await.auth, AuthMethod::IdentityFile { .. }),
            "{n} ist nicht auf Weg (a) zurückgefallen"
        );
    }
    // … und der, der ging, liegt im Schlüsselbund.
    assert!(matches!(
        f.server("gut").await.auth,
        AuthMethod::PrivateKey { .. }
    ));
}

#[tokio::test]
async fn t_3_1_9c_weg_c_ergibt_agent() {
    let plan = plan_from("Host a\n  IdentityFile /k/a\n");
    let f = Fixture::new();
    let spy = SpyKeyFiles::default();
    f.apply(&plan, &[choose(0, IdentityMode::Drop)], &spy)
        .await
        .expect("Import");
    assert!(spy.opened.lock().unwrap().is_empty(), "Weg (c) hat gelesen");
    assert_eq!(f.server("a").await.auth, AuthMethod::Agent);
}

// ------------------------------------------- §5.2 / §6.4.3: Einstellungen

#[tokio::test]
async fn t_6_4_3_importiertes_profil_hat_die_produktvorgaben() {
    let plan =
        plan_from("Host a\n  HostName 1.1.1.1\n  PostIngestPolicy allow\n  AiInjectionCheck no\n");
    let f = Fixture::new();
    f.apply(&plan, &[], &SpyKeyFiles::default())
        .await
        .expect("Import");
    let s = f.server("a").await;
    // §3.1.12: ausschließlich die Vorgabewerte des Produkts.
    assert_eq!(
        s.post_ingest_policy,
        ssh_manager_core::profiles::PostIngestPolicy::default()
    );
    assert!(!s.ai_injection_check_enabled);
}

// ------------------------------------------- §6.4.3a: Schlagwort abwählen

#[tokio::test]
async fn t_6_4_3a_abgewaehltes_schlagwort_landet_nicht_am_profil() {
    use ssh_manager_core::filter::{Pattern, Rule, RuleAction, RuleId, RuleOrigin, Scope};
    let rules = vec![Rule {
        id: RuleId("r-allow".into()),
        pattern: Pattern::Glob("systemctl restart *".into()),
        action: RuleAction::Allow,
        scope: Scope::Tag("*.prod.de".into()),
        priority: 0,
        origin: RuleOrigin::User,
    }];
    let plan = plan_from_inv(
        "Host *.prod.de\n  User deploy\nHost web1.prod.de\n",
        &[],
        &rules,
    );
    // Die Vorschau kennzeichnet das Schlagwort (§5.2a) …
    assert!(!plan.entries[0].tags[0].matched_rules.is_empty());

    // … und wählt der Nutzer es ab, trägt das Profil es nicht.
    let f = Fixture::new();
    f.apply(
        &plan,
        &[EntryChoice {
            index: 0,
            selected: true,
            identity_mode: IdentityMode::default(),
            rename_to: None,
            dropped_tags: vec!["*.prod.de".to_string()],
        }],
        &SpyKeyFiles::default(),
    )
    .await
    .expect("Import");
    assert!(
        f.server("web1.prod.de").await.tags.is_empty(),
        "abgewähltes Schlagwort ist doch am Profil"
    );
}

#[tokio::test]
async fn t_5_2a_nicht_abgewaehltes_schlagwort_bleibt() {
    // Gegenprobe: ohne sie wäre „Schlagworte immer weglassen" grün.
    let plan = plan_from("Host *.prod.de\n  User deploy\nHost web1.prod.de\n");
    let f = Fixture::new();
    f.apply(&plan, &[], &SpyKeyFiles::default())
        .await
        .expect("Import");
    assert_eq!(f.server("web1.prod.de").await.tags, vec!["*.prod.de"]);
}

// ------------------------------------------- §6.3.4/5: Konflikte, zweiter Import

#[tokio::test]
async fn t_6_3_5_namenskonflikt_wird_uebersprungen_bestand_unveraendert() {
    let f = Fixture::new();
    // Erster Import.
    let plan = plan_from("Host a\n  HostName 1.1.1.1\n  User max\n");
    f.apply(&plan, &[], &SpyKeyFiles::default())
        .await
        .expect("erster Import");
    let before = f.server("a").await;

    // Zweiter Import derselben Datei — jetzt mit dem Bestand als Inventar.
    let existing = f.servers().await;
    let plan2 = plan_from_inv("Host a\n  HostName 1.1.1.1\n  User max\n", &existing, &[]);
    assert!(
        plan2.entries[0].conflict.is_some(),
        "Konflikt nicht erkannt"
    );
    let out = f
        .apply(&plan2, &[], &SpyKeyFiles::default())
        .await
        .expect("zweiter Import");

    // §3.1.10: **nichts** angelegt — und auch keine zweite Gruppe.
    assert_eq!(out.created_servers, 0);
    assert_eq!(out.created_groups, 0);
    assert_eq!(out.skipped_conflicts, 1);
    assert_eq!(f.servers().await.len(), 1);
    // Das bestehende Profil ist Feld für Feld unverändert.
    assert_eq!(f.server("a").await, before);
}

#[tokio::test]
async fn t_3_1_8_konflikt_mit_neuem_namen_wird_angelegt() {
    let f = Fixture::new();
    let plan = plan_from("Host a\n  HostName 1.1.1.1\n");
    f.apply(&plan, &[], &SpyKeyFiles::default())
        .await
        .expect("erster");
    let existing = f.servers().await;
    let plan2 = plan_from_inv("Host a\n  HostName 1.1.1.1\n", &existing, &[]);
    let out = f
        .apply(
            &plan2,
            &[EntryChoice {
                index: 0,
                selected: true,
                identity_mode: IdentityMode::default(),
                rename_to: Some("a (importiert)".to_string()),
                dropped_tags: Vec::new(),
            }],
            &SpyKeyFiles::default(),
        )
        .await
        .expect("zweiter");
    assert_eq!(out.created_servers, 1);
    assert_eq!(f.servers().await.len(), 2);
    assert_eq!(f.server("a (importiert)").await.host, "1.1.1.1");
}

// ------------------------------------------- §6.3.8: Abbruch

#[tokio::test]
async fn t_6_3_8_alle_abgewaehlt_legt_nichts_an() {
    let plan = plan_from("Host a\nHost b\n");
    let f = Fixture::new();
    let choices: Vec<EntryChoice> = (0..2)
        .map(|i| EntryChoice {
            index: i,
            selected: false,
            identity_mode: IdentityMode::default(),
            rename_to: None,
            dropped_tags: Vec::new(),
        })
        .collect();
    let out = f
        .apply(&plan, &choices, &SpyKeyFiles::default())
        .await
        .expect("Import");
    assert_eq!(out.created_servers, 0);
    // §3.1.10: keine Gruppe, wenn kein Server übrig bleibt.
    assert_eq!(out.created_groups, 0);
    assert!(f.servers().await.is_empty());
    assert!(f.store.list_groups().await.unwrap().is_empty());
}

// ------------------------------------------- §6.3.9: Rollback

#[tokio::test]
async fn t_6_3_9_fehler_mitten_im_anlegen_nimmt_profile_und_gruppen_zurueck() {
    let plan = plan_from("Host a\nHost b\nHost c\n");
    let f = Fixture::new();
    // Der dritte Insert scheitert.
    f.store.fail_create_server_after(2);

    let err = f
        .apply(&plan, &[], &SpyKeyFiles::default())
        .await
        .expect_err("sollte scheitern");
    assert!(
        err.message.contains("Es wurde nichts angelegt"),
        "Meldung: {}",
        err.message
    );
    // §3.1.11: kein Profil **und keine Gruppe** dieses Laufs bleibt übrig.
    assert!(
        f.servers().await.is_empty(),
        "übrig: {:?}",
        f.servers().await
    );
    assert!(
        f.store.list_groups().await.unwrap().is_empty(),
        "Gruppe übrig geblieben"
    );
}

/// §3.1.11 und die Zusage aus `servers::create_server` („Kein verwaister
/// Eintrag für eine `ServerId`, die es nicht (mehr) gibt"): Nach einem
/// Fehler darf auf Weg (b) **kein** Schlüssel im Schlüsselbund
/// zurückbleiben.
///
/// Der andere Rollback-Test fährt Weg (a), wo nichts geschrieben wird, und
/// konnte diesen Fund aus Review-Runde 1 deshalb nicht sehen.
#[tokio::test]
async fn t_3_1_11_rollback_raeumt_auch_den_schluesselbund() {
    let plan = plan_from(
        "Host a\n  IdentityFile /k/a\nHost b\n  IdentityFile /k/b\nHost c\n  IdentityFile /k/c\n",
    );
    let f = Fixture::new();
    let spy = SpyKeyFiles::default();
    let choices: Vec<EntryChoice> = (0..3)
        .map(|i| choose(i, IdentityMode::IntoKeychain))
        .collect();
    // Der dritte Aufruf scheitert — zwei Schlüssel liegen dann schon im
    // Schlüsselbund.
    f.store.fail_create_server_after(3);

    f.apply(&plan, &choices, &spy)
        .await
        .expect_err("sollte scheitern");

    // Profile und Gruppen sind weg …
    assert!(f.servers().await.is_empty());
    assert!(f.store.list_groups().await.unwrap().is_empty());
    // … und der Schlüsselbund ist leer. Ohne das Abräumen lägen hier zwei
    // private Schlüssel unter ServerIds, die es nicht mehr gibt.
    let left = f.creds.secrets.lock().unwrap();
    assert!(
        left.is_empty(),
        "verwaiste Schlüsselbund-Einträge: {:?}",
        left.keys().collect::<Vec<_>>()
    );
}

// ------------------------------------------- Jump-Host

#[tokio::test]
async fn t_3_1_6_jump_host_zeigt_auf_das_importierte_profil() {
    let plan = plan_from("Host bastion\n  HostName 1.1.1.1\nHost x\n  ProxyJump bastion\n");
    let f = Fixture::new();
    f.apply(&plan, &[], &SpyKeyFiles::default())
        .await
        .expect("Import");
    let bastion = f.server("bastion").await;
    assert_eq!(f.server("x").await.jump_host, Some(bastion.id));
}

#[tokio::test]
async fn t_3_1_6_abgewaehltes_jump_ziel_ergibt_keinen_jump_host() {
    let plan = plan_from("Host bastion\n  HostName 1.1.1.1\nHost x\n  ProxyJump bastion\n");
    let f = Fixture::new();
    // Das Ziel abwählen — `x` darf dann keinen Jump-Host bekommen statt auf
    // etwas zu zeigen, das nicht existiert.
    let bastion_idx = plan
        .entries
        .iter()
        .position(|e| e.name == "bastion")
        .unwrap();
    f.apply(
        &plan,
        &[EntryChoice {
            index: bastion_idx,
            selected: false,
            identity_mode: IdentityMode::default(),
            rename_to: None,
            dropped_tags: Vec::new(),
        }],
        &SpyKeyFiles::default(),
    )
    .await
    .expect("Import");
    assert_eq!(f.server("x").await.jump_host, None);
}

// ------------------------------------------- §6.3.15: relativer Pfad

#[tokio::test]
async fn t_6_3_15_markierter_pfad_wird_trotzdem_angelegt() {
    let plan = plan_from("Host rel\n  IdentityFile keys/id_rsa\n");
    assert!(
        !plan.entries[0]
            .identity_file
            .as_ref()
            .unwrap()
            .usable_when_connecting,
        "hätte als nicht benutzbar markiert sein müssen"
    );
    let f = Fixture::new();
    f.apply(&plan, &[], &SpyKeyFiles::default())
        .await
        .expect("Import");
    // Trotzdem angelegt, mit dem Pfad unverändert (§3.1.9 a).
    match &f.server("rel").await.auth {
        AuthMethod::IdentityFile { path, .. } => assert_eq!(path, "keys/id_rsa"),
        other => panic!("erwartet IdentityFile, war {other:?}"),
    }
}

// ------------------------------------------- DiskKeyFiles

/// §5.5: Weg (b) benutzt **denselben** Leser wie die Handstelle, also den
/// echten [`crate::key_files::OsKeyFileReader`] mit allen Prüfungen aus
/// Spec 0076 — nicht eine eigene, schwächere Fassung.
fn disk() -> DiskKeyFiles<'static> {
    // `Box::leak`: Der Leser ist zustandslos und lebt im Betrieb ohnehin so
    // lange wie der `AppState`; im Test spart das eine Hilfsstruktur.
    let reader: &'static crate::key_files::OsKeyFileReader =
        Box::leak(Box::new(crate::key_files::OsKeyFileReader::new()));
    DiskKeyFiles { reader }
}

#[test]
fn t_5_5_weg_b_benutzt_dieselben_pruefungen_wie_die_handstelle() {
    let d = tempfile::tempdir().unwrap();

    // (1) Fehlende Datei.
    let missing = d.path().join("gibtsnicht");
    assert_eq!(
        disk().read_key(&missing.display().to_string()),
        Err(IdentityFallbackReason::Missing)
    );

    // (2) **Der Fund aus Review-Runde 1:** eine gewöhnliche Textdatei, die
    // die Zeichenfolge `PRIVATE KEY` enthält, aber kein gültiger Schlüssel
    // ist. Die erste Fassung prüfte nur diesen Teilstring und hätte den
    // Inhalt als „privaten Schlüssel" in den Schlüsselbund gelegt.
    let fake = d.path().join("notizen.txt");
    std::fs::write(
        &fake,
        "-----BEGIN OPENSSH PRIVATE KEY-----\nGEHEIMER_TEXT_4711\n-----END OPENSSH PRIVATE KEY-----\n",
    )
    .unwrap();
    let err = disk()
        .read_key(&fake.display().to_string())
        .expect_err("kein gültiger Schlüssel");
    assert_eq!(err, IdentityFallbackReason::NotAKey);
    // Der Grund trägt keinen Dateiinhalt (§5.1).
    assert!(!format!("{err:?}").contains("GEHEIMER_TEXT"));

    // (3) Ein relativer Pfad ist beim Lesen nicht auflösbar (Spec 0076 A-3).
    assert_eq!(
        disk().read_key("keys/id_rsa"),
        Err(IdentityFallbackReason::PathNotAbsolute)
    );
}

/// §5.6 / Review-Runde 1: Ein FIFO am `IdentityFile`-Pfad darf das Kommando
/// nicht unbegrenzt hängen lassen. Die erste Fassung benutzte
/// `fs::read_to_string` ohne `O_NONBLOCK` und ohne Prüfung der Dateiart.
#[cfg(unix)]
#[test]
fn t_5_5_fifo_haengt_nicht() {
    let d = tempfile::tempdir().unwrap();
    let fifo = d.path().join("pipe");
    let c = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: gültiger, nullterminierter Pfad in einem frischen Tempdir.
    let rc = unsafe { libc::mkfifo(c.as_ptr(), 0o600) };
    assert_eq!(rc, 0, "mkfifo fehlgeschlagen");

    // Ohne Leser-seitige Prüfung blockiert schon das Öffnen — der Test
    // käme nie zurück. Mit ihr ist es ein Befund.
    let err = disk()
        .read_key(&fifo.display().to_string())
        .expect_err("FIFO ist kein Schlüssel");
    assert_eq!(err, IdentityFallbackReason::NotARegularFile);
}

// ------------------------- `build_preview_dto`: ADR-Nachtrag P10 Nr. 2 ---
//
// Drei Lücken, die ADR 0074 (Schritte 0–3) ausdrücklich als Vorbedingung
// für Schritt 5 benannt hatte: Herkunft der Werte fehlte im DTO ganz,
// `Conflict::kind` kam nicht durch, und ein Bestands-Jump-Ziel zeigte nur
// den Platzhaltertext `"(Bestand)"` statt seines echten Namens. `core`
// kannte alle drei Tatsachen schon vorher — hier wird geprüft, dass sie
// jetzt tatsächlich im DTO ankommen.

fn inventory_server(
    name: &str,
    host: &str,
    port: u16,
    user: &str,
) -> ssh_manager_core::profiles::Server {
    ssh_manager_core::profiles::Server {
        id: ssh_manager_core::shared::ServerId::new(),
        name: name.to_string(),
        host: host.to_string(),
        port,
        username: user.to_string(),
        group_id: None,
        tags: vec![],
        auth: AuthMethod::Agent,
        notes: String::new(),
        jump_host: None,
        post_ingest_policy: Default::default(),
        ai_injection_check_enabled: false,
        sftp_server_path: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

#[test]
fn t_p10_2_dto_traegt_herkunft_der_werte() {
    let plan = plan_from("Host web1\n  HostName 10.0.0.1\n  Port 2222\n  User max\n");
    let dto = build_preview_dto(&plan, &[], &[]);
    let e = &dto.entries[0];

    assert_eq!(e.host.value, "10.0.0.1");
    let origin = e.host.origin.as_ref().expect("HostName kam aus der Datei");
    assert_eq!(origin.line, 2);
    assert_eq!(origin.file, "/tmp/config");

    // Vorgabewert (kein `User` in der Datei) ⇒ `origin: None`.
    let plan_no_user = plan_from("Host web1\n  HostName 10.0.0.1\n");
    let dto_no_user = build_preview_dto(&plan_no_user, &[], &[]);
    assert!(dto_no_user.entries[0].username.origin.is_none());
    assert_eq!(dto_no_user.entries[0].username.value, "");
}

#[test]
fn t_p10_2_dto_traegt_conflict_kind() {
    let existing = inventory_server("web1", "9.9.9.9", 22, "root");
    let plan = plan_from_inv(
        "Host web1\n  HostName 10.0.0.1\n",
        std::slice::from_ref(&existing),
        &[],
    );
    let dto = build_preview_dto(&plan, &[], std::slice::from_ref(&existing));
    let conflict = dto.entries[0].conflict.as_ref().expect("Namenskonflikt");
    assert_eq!(conflict.kind, "name");
    assert_eq!(conflict.existing_name, "web1");

    let existing2 = inventory_server("other", "10.0.0.5", 22, "root");
    let plan2 = plan_from_inv(
        "Host web2\n  HostName 10.0.0.5\n  Port 22\n  User root\n",
        std::slice::from_ref(&existing2),
        &[],
    );
    let dto2 = build_preview_dto(&plan2, &[], std::slice::from_ref(&existing2));
    let conflict2 = dto2.entries[0].conflict.as_ref().expect("Adresskonflikt");
    assert_eq!(conflict2.kind, "address");
}

#[test]
fn t_p10_2_dto_zeigt_echten_namen_des_bestands_jump_ziels() {
    let bastion = inventory_server("prod-bastion", "10.0.0.9", 22, "ops");
    let plan = plan_from_inv(
        "Host web1\n  HostName 10.0.0.1\n  ProxyJump prod-bastion\n",
        std::slice::from_ref(&bastion),
        &[],
    );
    let dto = build_preview_dto(&plan, &[], std::slice::from_ref(&bastion));
    let jump = dto.entries[0]
        .jump_host
        .as_ref()
        .expect("Jump-Ziel im Bestand");
    // Vorher stand hier der Platzhaltertext "(Bestand)" — jetzt der
    // tatsächliche `Server::name`.
    assert_eq!(jump.name, "prod-bastion");
    assert!(jump.existing);
}

#[test]
fn t_p10_2_dto_kennzeichnet_geplantes_jump_ziel_als_nicht_bestand() {
    let plan = plan_from(
        "Host bastion\n  HostName 10.0.0.9\nHost web1\n  HostName 10.0.0.1\n  ProxyJump bastion\n",
    );
    let dto = build_preview_dto(&plan, &[], &[]);
    let web1 = dto.entries.iter().find(|e| e.name == "web1").unwrap();
    let jump = web1.jump_host.as_ref().expect("Jump-Ziel im selben Import");
    assert_eq!(jump.name, "bastion");
    assert!(!jump.existing);
}

/// Belegt das JSON-Vokabular, auf das sich das Frontend (`types.ts`)
/// verlässt — ohne diesen Test wäre eine stillschweigende Umbenennung einer
/// `IdentityFallbackReason`-Variante erst zur Laufzeit im Frontend
/// bemerkbar (falsche/nicht übersetzte Fehlermeldung), nicht am Gate.
#[test]
fn identity_fallback_reason_serializes_to_the_strings_the_frontend_expects() {
    let cases = [
        (IdentityFallbackReason::Missing, "\"missing\""),
        (IdentityFallbackReason::Unreadable, "\"unreadable\""),
        (IdentityFallbackReason::NotAKey, "\"notAKey\""),
        (
            IdentityFallbackReason::NotARegularFile,
            "\"notARegularFile\"",
        ),
        (IdentityFallbackReason::TooLarge, "\"tooLarge\""),
        (
            IdentityFallbackReason::PathNotAbsolute,
            "\"pathNotAbsolute\"",
        ),
    ];
    for (reason, expected) in cases {
        assert_eq!(serde_json::to_string(&reason).unwrap(), expected);
    }
}
