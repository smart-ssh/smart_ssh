//! Den bestätigten Importplan ausführen (Spec 0075, §7.3).
//!
//! Dies ist der einzige Schritt, in dem überhaupt eine **Schlüsseldatei**
//! geöffnet wird — und nur auf Weg (b) aus §3.1.9, nur beim Bestätigen, und
//! nur für die Pfade, die schon im Plan standen (§5.1). Die Vorschau öffnet
//! nichts.
//!
//! **Alles oder nichts** (§3.1.11): Schlägt ein Schritt fehl, werden die in
//! diesem Lauf erzeugten Profile **und Gruppen** wieder abgeräumt.

use chrono::Utc;
use secrecy::ExposeSecret;
use ssh_manager_core::profiles::ssh_config::{ImportPlan, JumpTarget};
use ssh_manager_core::profiles::{CredentialStore, Group, GroupId, ProfileStore};
use ssh_manager_core::shared::ServerId;
use ssh_manager_core::ssh::{KeyFileError, KeyFileReader};

use crate::dto::{AuthMethodInput, ServerInput};
use crate::error::{CommandError, CommandResult};

/// Was mit `IdentityFile` geschehen soll (§3.1.9). Gilt für den ganzen
/// Import und ist je Eintrag umstellbar; Vorgabe ist [`Self::KeepAsFile`],
/// weil nur dieser Weg keine einzige zusätzliche Datei öffnet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IdentityMode {
    /// (a) Als Schlüsseldatei übernehmen — die Datei bleibt **ungeöffnet**.
    #[default]
    KeepAsFile,
    /// (b) Einlesen und in den Schlüsselbund legen.
    IntoKeychain,
    /// (c) Nicht übernehmen — der Server bekommt `Agent`.
    Drop,
}

/// Die Wahl des Nutzers zu **einem** Eintrag der Vorschau.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryChoice {
    /// Index in [`ImportPlan::entries`].
    pub index: usize,
    /// Abgewählt ⇒ dieser Eintrag entsteht nicht (§3.1.7).
    #[serde(default = "yes")]
    pub selected: bool,
    #[serde(default)]
    pub identity_mode: IdentityMode,
    /// §3.1.8: Bei einem Konflikt entweder überspringen (Vorgabe) oder
    /// unter diesem Namen neu anlegen.
    #[serde(default)]
    pub rename_to: Option<String>,
    /// §5.2a: Schlagworte, die der Nutzer abgewählt hat — etwa weil sie
    /// eine bestehende Filterregel treffen.
    #[serde(default)]
    pub dropped_tags: Vec<String>,
}

fn yes() -> bool {
    true
}

/// Was beim Ausführen geschah — Grundlage der Abschlussmeldung.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyOutcome {
    pub created_servers: usize,
    pub created_groups: usize,
    pub skipped_conflicts: usize,
    /// §3.1.9 (b): Einträge, die auf Weg (a) zurückgefallen sind, mit
    /// Grund — **nie** mit Dateiinhalt.
    pub identity_fallbacks: Vec<IdentityFallback>,
    /// §3.1.9 (b), letzter Punkt: Einträge, deren Schlüssel auf Weg (b)
    /// **verschlüsselt** übernommen wurde — die Vorschau kann das nicht
    /// vorher wissen (§5.1: eine Schlüsseldatei wird nur beim Bestätigen
    /// geöffnet, nie in der Vorschau), deshalb steht es erst hier, nach dem
    /// tatsächlichen Lesen. Die Meldung dazu MUSS sagen, dass die
    /// Passphrase nachzutragen ist, bevor die erste Verbindung gelingt
    /// (spec-reviewer-Fund, Runde 1: fehlte bisher ganz).
    pub identity_encrypted: Vec<IdentityEncryptedNotice>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityEncryptedNotice {
    pub entry: String,
    pub path: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityFallback {
    pub entry: String,
    pub path: String,
    pub reason: IdentityFallbackReason,
}

/// Warum ein Eintrag auf Weg (a) zurueckgefallen ist (§3.1.9 b).
///
/// Die Varianten spiegeln [`KeyFileError`] — der Import erfindet keine
/// eigenen Kategorien und vor allem keine eigenen, schwaecheren Pruefungen
/// (§5.5). Traegt **nie** Dateiinhalt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum IdentityFallbackReason {
    Missing,
    Unreadable,
    NotAKey,
    /// Spec 0076: keine gewoehnliche Datei (etwa ein FIFO) — ohne diese
    /// Pruefung haenge das Kommando an einer Pipe unbegrenzt.
    NotARegularFile,
    /// Spec 0076: ueber der Groessengrenze.
    TooLarge,
    /// §3.1.9 (a) haette es markiert; beim Lesen ist ein nicht absoluter
    /// Pfad nicht aufloesbar.
    PathNotAbsolute,
}

impl From<&KeyFileError> for IdentityFallbackReason {
    fn from(e: &KeyFileError) -> Self {
        match e {
            KeyFileError::NotFound { .. } => IdentityFallbackReason::Missing,
            KeyFileError::NotReadable { .. } => IdentityFallbackReason::Unreadable,
            // §5.5: Fuer den Import ist ein Rechte-Mangel ein **Befund**,
            // keine Sperre — `enforce_permissions = false`. Kaeme die
            // Variante doch, waere "nicht lesbar" die ehrliche Auskunft.
            KeyFileError::PermissionsTooOpen { .. } => IdentityFallbackReason::Unreadable,
            KeyFileError::TooLarge { .. } => IdentityFallbackReason::TooLarge,
            KeyFileError::NotARegularFile { .. } => IdentityFallbackReason::NotARegularFile,
            KeyFileError::PathNotAbsolute { .. } => IdentityFallbackReason::PathNotAbsolute,
            KeyFileError::InvalidKey { .. } => IdentityFallbackReason::NotAKey,
        }
    }
}

/// Ergebnis eines erfolgreichen Lesens auf Weg (b) — der Inhalt für den
/// Schlüsselbund plus, ob der Schlüssel verschlüsselt war (Grundlage für
/// [`IdentityEncryptedNotice`], §3.1.9 b).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyFileRead {
    pub content: String,
    pub encrypted: bool,
}

/// Liest eine Schlüsseldatei für Weg (b). Getrennt als Trait, damit die
/// Tests belegen können, **welche** Dateien geöffnet wurden — und dass die
/// Vorschau keine öffnet (§6.4.1b).
///
/// `Send + Sync`: Die Quelle wird über ein `await` hinweg gehalten (das
/// Anlegen jedes Profils ist async), und ohne diese Schranke wäre das
/// Future des Tauri-Kommandos nicht `Send`.
pub trait KeyFileSource: Send + Sync {
    fn read_key(&self, path: &str) -> Result<KeyFileRead, IdentityFallbackReason>;
}

/// Führt den bestätigten Plan aus (§3.1.11).
///
/// `choices` ist die Wahl des Nutzers je Eintrag; ein Eintrag ohne Wahl
/// gilt als ausgewählt mit den Vorgaben. Konflikte werden **übersprungen**,
/// solange kein `rename_to` gesetzt ist (§3.1.8, Vorgabe).
pub async fn apply_import(
    plan: &ImportPlan,
    choices: &[EntryChoice],
    store: &dyn ProfileStore,
    credential_store: &(dyn CredentialStore + Send + Sync),
    keychain: credentials_keyring::KeychainAvailability,
    keys: &dyn KeyFileSource,
) -> CommandResult<ApplyOutcome> {
    let choice_of = |i: usize| -> EntryChoice {
        choices
            .iter()
            .find(|c| c.index == i)
            .cloned()
            .unwrap_or(EntryChoice {
                index: i,
                selected: true,
                identity_mode: IdentityMode::default(),
                rename_to: None,
                dropped_tags: Vec::new(),
            })
    };

    // §4.3: "Ist er leer ..., wird **kein Profil angelegt**." Das gilt auch
    // fuer den Namen, den der Nutzer bei einem Konflikt eingibt - er ist der
    // einzige Freitext, der von aussen in ein erzeugtes Profil gelangt.
    // Vorher wurde er ungetrimmt und auch leer uebernommen (Review-Runde 1),
    // womit der Import genau die namenlosen Profile in Serie erzeugen konnte,
    // die §4.3 ausschliesst.
    let clean_rename = |c: &EntryChoice| -> Option<String> {
        let name = c.rename_to.as_ref()?.trim().to_string();
        if name.is_empty() {
            None
        } else {
            Some(name)
        }
    };

    // Welche Eintraege entstehen wirklich? §3.1.8: ein Konflikt wird
    // uebersprungen, wenn der Nutzer keinen brauchbaren neuen Namen hat.
    let mut take = vec![false; plan.entries.len()];
    let mut outcome = ApplyOutcome::default();
    for (i, e) in plan.entries.iter().enumerate() {
        let c = choice_of(i);
        if !c.selected {
            continue;
        }
        if e.conflict.is_some() && clean_rename(&c).is_none() {
            outcome.skipped_conflicts += 1;
            continue;
        }
        take[i] = true;
    }

    // §4.5 und §3.1.10: Nur Gruppen anlegen, die wirklich einen Server
    // bekommen — samt ihrer Elternkette. Ein zweiter Import derselben Datei
    // legt deshalb **auch keine Gruppe** an, weil dann kein Eintrag übrig
    // bleibt.
    let mut group_needed = vec![false; plan.groups.len()];
    for (i, e) in plan.entries.iter().enumerate() {
        if !take[i] {
            continue;
        }
        let mut g = Some(e.group);
        while let Some(idx) = g {
            if group_needed[idx] {
                break;
            }
            group_needed[idx] = true;
            g = plan.groups[idx].parent;
        }
    }

    // ---- Anlegen, mit Rücknahme bei jedem Fehler ---------------------
    let mut made_groups: Vec<GroupId> = Vec::new();
    let mut made_servers: Vec<ServerId> = Vec::new();
    let mut group_ids: Vec<Option<GroupId>> = vec![None; plan.groups.len()];

    // Eltern vor Kindern — `plan.groups` ist schon so sortiert (§3.1.11).
    for (i, g) in plan.groups.iter().enumerate() {
        if !group_needed[i] {
            continue;
        }
        let parent_id = match g.parent {
            Some(p) => group_ids[p],
            None => None,
        };
        let now = Utc::now();
        let group = Group {
            id: GroupId::new(),
            name: g.name.clone(),
            parent_id,
            notes: String::new(),
            created_at: now,
            updated_at: now,
        };
        if let Err(err) = store.create_group(&group).await {
            rollback(store, credential_store, &made_servers, &made_groups).await;
            return Err(err.into());
        }
        group_ids[i] = Some(group.id);
        made_groups.push(group.id);
        outcome.created_groups += 1;
    }

    // Jump-Ziele vor ihren Nutzern — auch das steht schon in der
    // Reihenfolge von `plan.entries` (§3.1.11).
    let mut server_ids: Vec<Option<ServerId>> = vec![None; plan.entries.len()];
    for (i, e) in plan.entries.iter().enumerate() {
        if !take[i] {
            continue;
        }
        let c = choice_of(i);

        // §3.1.9: Der Weg entscheidet, ob überhaupt eine Datei aufgeht.
        let auth = match (&e.identity_file, c.identity_mode) {
            (Some(idf), IdentityMode::IntoKeychain) => match keys.read_key(&idf.path) {
                Ok(read) => {
                    // §3.1.9 (b), letzter Punkt: Die Vorschau konnte das
                    // nicht wissen (§5.1) — jetzt, nach dem tatsächlichen
                    // Lesen, steht es in der Abschlussmeldung, damit der
                    // Nutzer die Passphrase nachträgt, bevor er sich zum
                    // ersten Mal verbindet.
                    if read.encrypted {
                        outcome.identity_encrypted.push(IdentityEncryptedNotice {
                            entry: e.name.clone(),
                            path: idf.path.clone(),
                        });
                    }
                    AuthMethodInput::PrivateKey {
                        key_content: Some(read.content),
                        // §3.1.9 (b): Nach einer Passphrase wird beim Import
                        // **nicht** gefragt; ein verschlüsselter Schlüssel
                        // geht byte-gleich in den Schlüsselbund (Spec 0076,
                        // C-4).
                        passphrase: None,
                    }
                }
                Err(reason) => {
                    // Ein Fehler lässt den Import **nicht** scheitern:
                    // dieser eine Server fällt auf Weg (a) zurück, und die
                    // Meldung sagt warum (§3.1.9 b). Der Grund trägt nie
                    // Dateiinhalt.
                    outcome.identity_fallbacks.push(IdentityFallback {
                        entry: e.name.clone(),
                        path: idf.path.clone(),
                        reason,
                    });
                    AuthMethodInput::IdentityFile {
                        path: idf.path.clone(),
                        passphrase: None,
                    }
                }
            },
            // (a) — der Pfad wandert unverändert in die Anmeldeart, die
            // Datei bleibt zu (§4.6, §5.5).
            (Some(idf), IdentityMode::KeepAsFile) => AuthMethodInput::IdentityFile {
                path: idf.path.clone(),
                passphrase: None,
            },
            // (c) und „kein IdentityFile" — beides `Agent`.
            (Some(_), IdentityMode::Drop) | (None, _) => AuthMethodInput::Agent,
        };

        let jump_host = match e.jump {
            Some(JumpTarget::Existing(id)) => Some(id),
            // Zeigt das Ziel auf einen Eintrag, der abgewählt wurde, dann
            // entsteht **kein** Jump-Host — lieber ohne als auf etwas, das
            // es nicht gibt.
            Some(JumpTarget::Planned(t)) => server_ids.get(t).copied().flatten(),
            None => None,
        };

        let tags: Vec<String> = e
            .tags
            .iter()
            .map(|t| t.tag.clone())
            .filter(|t| !c.dropped_tags.contains(t))
            .collect();

        let input = ServerInput {
            name: clean_rename(&c).unwrap_or_else(|| e.name.clone()),
            host: e.host.value.clone(),
            port: e.port.value,
            username: e.username.value.clone(),
            group_id: group_ids[e.group],
            tags,
            auth,
            jump_host,
            sudo_password: None,
            // §3.1.12 / §5.2: **ausschließlich** die Vorgabewerte des
            // Produkts. Es gibt hier bewusst keinen Weg, sie aus der Datei
            // zu setzen — kein Feld, keine Direktive, kein Kommentar.
            post_ingest_policy: Default::default(),
            ai_injection_check_enabled: false,
            sftp_server_path: None,
        };

        match crate::servers::create_server(store, credential_store, keychain, input).await {
            Ok(id) => {
                server_ids[i] = Some(id);
                made_servers.push(id);
                outcome.created_servers += 1;
            }
            Err(err) => {
                rollback(store, credential_store, &made_servers, &made_groups).await;
                return Err(CommandError {
                    message: format!(
                        "Eintrag „{}“ konnte nicht angelegt werden: {}. Es wurde nichts angelegt.",
                        e.name, err.message
                    ),
                    code: err.code,
                    feature_locked: None,
                });
            }
        }
    }

    Ok(outcome)
}

/// §3.1.11: Profile **und Gruppen** dieses Laufs zurücknehmen. Server
/// zuerst — `groups.parent_id`/`servers.group_id` sind Fremdschlüssel, und
/// Gruppen in umgekehrter Anlegereihenfolge, damit ein Kind vor seinem
/// Elternteil verschwindet.
async fn rollback(
    store: &dyn ProfileStore,
    credential_store: &(dyn CredentialStore + Send + Sync),
    servers: &[ServerId],
    groups: &[GroupId],
) {
    for id in servers.iter().rev() {
        // **Erst der Schluesselbund, dann die Zeile.** Auf Weg (b) hat
        // `create_server` den Schluesselinhalt schon unter
        // `server:<id>:private_key` abgelegt; nur das Profil zu loeschen
        // liesse ihn als verwaisten Eintrag zu einer `ServerId` zurueck, die
        // es nicht mehr gibt - genau das, was `servers::create_server` fuer
        // seinen eigenen Fehlerpfad ausdruecklich zusichert. Eine erste
        // Fassung dieses Rollbacks tat das (Review-Runde 1); der
        // Rollback-Test fuhr Weg (a) und konnte es nicht sehen.
        crate::server_credentials::delete_all_possible_server_secrets(credential_store, *id);
        // Ein Fehler beim Aufräumen darf den ursprünglichen Fehler nicht
        // verdecken — er wird vermerkt, nicht weitergeworfen.
        if let Err(err) = store.delete_server(id).await {
            tracing::error!(
                target: "ssh_config_import",
                error = %err,
                "Rücknahme eines importierten Profils fehlgeschlagen"
            );
        }
    }
    for id in groups.iter().rev() {
        if let Err(err) = store.delete_group(id).await {
            tracing::error!(
                target: "ssh_config_import",
                error = %err,
                "Rücknahme einer importierten Gruppe fehlgeschlagen"
            );
        }
    }
}

/// Weg (b) im Betrieb - ueber **denselben** [`KeyFileReader`], den das
/// Anlegen eines Servers von Hand benutzt.
///
/// §5.5 ist hier woertlich zu nehmen: "dann gelten die Pruefungen aus Spec
/// 0076 (Dateirechte, symbolische Links, Gueltigkeit des Schluessels), und
/// zwar dieselben wie beim Anlegen eines Servers von Hand. Der Import
/// erfindet dafuer keine eigenen, schwaecheren Regeln."
///
/// Die erste Fassung tat genau das - `exists()` + `read_to_string` +
/// Teilstring-Suche nach `PRIVATE KEY` - und war damit an drei Stellen
/// schwaecher als die Handstelle (Review-Runde 1): keine Groessengrenze,
/// keine Pruefung der Dateiart (ein FIFO haette das Kommando unbegrenzt
/// haengen lassen), und eine Teilstring-Pruefung statt echter
/// Schluesselgueltigkeit, ueber die eine beliebige Textdatei mit dieser
/// Zeichenfolge als "privater Schluessel" in den Schluesselbund gewandert
/// waere.
///
/// `enforce_permissions: false` - fuer den Import ist ein Rechte-Mangel ein
/// **Befund**, keine Sperre; genau so sieht es der Trait-Kommentar zu
/// [`KeyFileReader::read`] fuer den Import ausdruecklich vor.
///
/// §5.1: Geoeffnet wird **nur**, was im Plan stand. Diese Quelle bekommt
/// deshalb den Pfad aus dem Plan, nie einen aus dem Frontend.
pub struct DiskKeyFiles<'a> {
    pub reader: &'a (dyn KeyFileReader + Send + Sync),
}

impl KeyFileSource for DiskKeyFiles<'_> {
    fn read_key(&self, path: &str) -> Result<KeyFileRead, IdentityFallbackReason> {
        match self.reader.read(path, false) {
            // §3.1.9 (b) / Spec 0076 C-4: Der Dateiinhalt geht byte-gleich
            // in den Schluesselbund, auch wenn er verschluesselt ist - nach
            // einer Passphrase wird beim Import nicht gefragt. `encrypted`
            // wandert mit, damit die Abschlussmeldung sagen kann, dass die
            // Passphrase nachzutragen ist (spec-reviewer-Fund, Runde 1).
            Ok(content) => Ok(KeyFileRead {
                content: content.key.expose_secret().to_string(),
                encrypted: content.encrypted,
            }),
            // Kein Fehler laesst den Import scheitern: dieser eine Server
            // faellt auf Weg (a) zurueck, und die Meldung sagt warum
            // (§3.1.9 b). Der Grund traegt nie Dateiinhalt.
            Err(err) => Err(IdentityFallbackReason::from(&err)),
        }
    }
}

// ------------------------------------------------------ Tauri-Kommandos

/// Der Plan der letzten Vorschau, wie er im `AppState` liegt (§5.1).
///
/// Er wird **hier** gehalten und nicht im Frontend, damit beim Bestätigen
/// nur Indizes zurückkommen. Was die Vorschau nicht genannt hat, kann
/// dadurch nicht geöffnet werden — auch nicht, wenn sich die
/// Konfigurationsdatei zwischenzeitlich geändert hat.
///
/// Trägt bewusst **nur** den Plan: Die Dateiliste ist mit der Vorschau
/// schon beim Frontend und wird beim Bestätigen nicht mehr gebraucht — was
/// hier nicht liegt, kann auch nicht versehentlich erneut geöffnet werden.
#[derive(Debug, Clone)]
pub struct PendingImport {
    pub plan: ImportPlan,
}

/// Die Vorschau für das Frontend (§3.1.7). Trägt Pfade und Felder, aber
/// **keinen** Dateiinhalt (§5.4).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreviewDto {
    pub groups: Vec<PreviewGroupDto>,
    pub entries: Vec<PreviewEntryDto>,
    pub files: Vec<PreviewFileDto>,
    pub skipped: Vec<PreviewSkippedDto>,
    /// §3.1.9 (b): wie viele Dateien auf Weg (b) geöffnet **würden** —
    /// die Vorschau nennt sie vorher, sie öffnet sie nicht.
    pub identity_file_paths: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewGroupDto {
    pub name: String,
    pub parent: Option<usize>,
    pub source_path: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewEntryDto {
    pub index: usize,
    pub name: String,
    pub group: usize,
    pub host: SourcedDto<String>,
    pub port: SourcedDto<u16>,
    pub username: SourcedDto<String>,
    pub tags: Vec<PreviewTagDto>,
    pub identity_file: Option<PreviewIdentityFileDto>,
    pub jump_host: Option<PreviewJumpDto>,
    pub conflict: Option<PreviewConflictDto>,
}

/// Ein Feldwert mit seiner Herkunft (§3.1.7 „samt … Herkunft der Werte").
/// `origin: None` ⇒ Vorgabe des Produkts, nicht aus der Datei — spiegelt
/// [`ssh_manager_core::profiles::ssh_config::Sourced`] 1:1, nur
/// `camelCase` und ohne die interne `Provenance`-Struct direkt zu
/// serialisieren (ADR 0075).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcedDto<T> {
    pub value: T,
    pub origin: Option<OriginDto>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OriginDto {
    pub file: String,
    pub line: u32,
    /// Die `Host`-Angaben des Blocks, aus dem der Wert kommt (§3.1.3).
    pub block: String,
}

impl From<&ssh_manager_core::profiles::ssh_config::Provenance> for OriginDto {
    fn from(p: &ssh_manager_core::profiles::ssh_config::Provenance) -> Self {
        OriginDto {
            file: p.file.clone(),
            line: p.line,
            block: p.block.clone(),
        }
    }
}

fn sourced_dto<T: Clone>(s: &ssh_manager_core::profiles::ssh_config::Sourced<T>) -> SourcedDto<T> {
    SourcedDto {
        value: s.value.clone(),
        origin: s.origin.as_ref().map(OriginDto::from),
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewIdentityFileDto {
    pub path: String,
    pub origin: OriginDto,
    /// §3.1.9 (a): `false` ⇒ „beim Verbinden nicht benutzbar, absoluter
    /// Pfad nötig".
    pub usable_when_connecting: bool,
}

/// Wohin ein `jump_host` zeigt — mit dem **tatsächlichen** Namen des
/// Ziels, nicht mehr dem Platzhaltertext `"(Bestand)"` (ADR zu diesem
/// Schritt, P10 Nr. 2): Für ein Bestandsziel steht hier sein echter
/// `Server::name`, für ein geplantes Ziel der Alias, den es in diesem
/// Import bekäme.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewJumpDto {
    pub name: String,
    /// `true` ⇒ das Ziel liegt bereits im Bestand, `false` ⇒ es entsteht in
    /// diesem Import. Das Frontend braucht das, um z. B. beim Abwählen
    /// eines geplanten Ziels zu erklären, warum die Kante verschwindet
    /// (§3.1.6/ADR 0074 Punkt 8), während ein Bestandsziel davon unberührt
    /// bleibt.
    pub existing: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewConflictDto {
    /// `"name"` oder `"address"` (§3.1.8) — bisher fehlte diese
    /// Unterscheidung im DTO, obwohl `core` sie längst kennt (ADR zu diesem
    /// Schritt, P10 Nr. 2). Das Frontend braucht sie für die richtige
    /// Meldung („Name schon vergeben" vs. „Adresse/Port/Nutzer schon
    /// vergeben").
    pub kind: String,
    pub existing_name: String,
}

fn conflict_kind_key(k: ssh_manager_core::profiles::ssh_config::ConflictKind) -> &'static str {
    use ssh_manager_core::profiles::ssh_config::ConflictKind;
    match k {
        ConflictKind::Name => "name",
        ConflictKind::Address => "address",
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewTagDto {
    pub tag: String,
    pub origin: OriginDto,
    /// §5.2a: nicht leer ⇒ das Schlagwort trifft diese bestehenden Regeln.
    pub matched_rules: Vec<PreviewMatchedRuleDto>,
    /// Schlagwort ohne Platzhalter, aus einer buchstäblichen Angabe in einem
    /// gemischten Block — trifft eine Tag-Regel **exakt** und gehört deshalb
    /// deutlicher gekennzeichnet (§5.2a, offene Entscheidung Q-BL-0216-02).
    pub is_literal: bool,
}

/// §5.2a verlangt, „die betroffene Regel" zu nennen, nicht nur, dass eine
/// Regel getroffen wurde (spec-reviewer-Fund, Runde 1 — `action` fehlte
/// vorher ganz, das DTO warf `MatchedRule::action` weg). `action` ist das
/// sicherheitsrelevante Feld: Nur eine **Allow**-Regel kann ein importiertes
/// Profil von `Confirm` auf `Allow` heben (§5.2a) — eine `Deny`-Regel bleibt
/// ohnehin wirksam. `Rule` selbst hat keinen sprechenden Namen (nur Muster +
/// Aktion); eine vollständige Musteranzeige wäre eine weitere Ausbaustufe.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewMatchedRuleDto {
    pub rule_id: String,
    /// `"allow" | "confirm" | "deny"`.
    pub action: String,
}

fn rule_action_key(a: &ssh_manager_core::filter::RuleAction) -> &'static str {
    use ssh_manager_core::filter::RuleAction;
    match a {
        RuleAction::Allow => "allow",
        RuleAction::Confirm => "confirm",
        RuleAction::Deny => "deny",
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewFileDto {
    pub path: String,
    pub depth: u8,
    pub status: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewSkippedDto {
    pub file: String,
    pub line: u32,
    /// Der **Name** der Direktive — oder `null` für `unlesbare Zeile`
    /// (§3.1.5). Nie ein Wert.
    pub directive: Option<String>,
    pub reason: String,
    pub entry: Option<String>,
}

fn reason_key(r: &ssh_manager_core::profiles::ssh_config::SkipReason) -> String {
    use ssh_manager_core::profiles::ssh_config::{ProxyJumpRejection as P, SkipReason as R};
    match r {
        R::Unsupported => "unsupported",
        R::AlreadySet => "alreadySet",
        R::EmptyValue => "emptyValue",
        R::OutsideHostBlock => "outsideHostBlock",
        R::InvalidValue => "invalidValue",
        R::EmptyAlias => "emptyAlias",
        R::MixedWildcardBlock => "mixedWildcardBlock",
        R::IncludeTooDeep => "includeTooDeep",
        R::IncludeAlreadyRead => "includeAlreadyRead",
        R::IncludeUnreadable => "includeUnreadable",
        R::IncludeNotSshConfig => "includeNotSshConfig",
        R::IncludeNoMatch => "includeNoMatch",
        R::ProxyJump(p) => match p {
            P::Cycle => "proxyJumpCycle",
            P::HopUnresolved => "proxyJumpHopUnresolved",
            P::IntermediateExists => "proxyJumpIntermediateExists",
            P::AmbiguousPredecessor => "proxyJumpAmbiguousPredecessor",
            P::OwnProxyJumpAndIntermediate => "proxyJumpOwnAndIntermediate",
            P::LocalPseudoServer => "proxyJumpLocalPseudoServer",
        },
    }
    .to_string()
}

/// Baut die Vorschau aus Plan, Dateiliste und Bestand. Rein, damit sie
/// prüfbar ist. `servers` wird **nur** gelesen, um den echten Namen eines
/// Bestands-Jump-Ziels aufzulösen (`PreviewJumpDto`, ADR 0075) — vorher
/// stand dort der Platzhaltertext `"(Bestand)"`.
pub fn build_preview_dto(
    plan: &ImportPlan,
    files: &[crate::ssh_config_import::FileReport],
    servers: &[ssh_manager_core::profiles::Server],
) -> ImportPreviewDto {
    use ssh_manager_core::profiles::ssh_config::SkippedKind;

    let entries: Vec<PreviewEntryDto> = plan
        .entries
        .iter()
        .enumerate()
        .map(|(i, e)| PreviewEntryDto {
            index: i,
            name: e.name.clone(),
            group: e.group,
            host: sourced_dto(&e.host),
            port: sourced_dto(&e.port),
            username: sourced_dto(&e.username),
            tags: e
                .tags
                .iter()
                .map(|t| PreviewTagDto {
                    tag: t.tag.clone(),
                    origin: OriginDto::from(&t.origin),
                    matched_rules: t
                        .matched_rules
                        .iter()
                        .map(|m| PreviewMatchedRuleDto {
                            rule_id: m.rule_id.0.clone(),
                            action: rule_action_key(&m.action).to_string(),
                        })
                        .collect(),
                    is_literal: t.is_literal,
                })
                .collect(),
            identity_file: e.identity_file.as_ref().map(|idf| PreviewIdentityFileDto {
                path: idf.path.clone(),
                origin: OriginDto::from(&idf.origin),
                usable_when_connecting: idf.usable_when_connecting,
            }),
            jump_host: match e.jump {
                Some(JumpTarget::Planned(t)) => plan.entries.get(t).map(|x| PreviewJumpDto {
                    name: x.name.clone(),
                    existing: false,
                }),
                Some(JumpTarget::Existing(id)) => {
                    servers.iter().find(|s| s.id == id).map(|s| PreviewJumpDto {
                        name: s.name.clone(),
                        existing: true,
                    })
                }
                None => None,
            },
            conflict: e.conflict.as_ref().map(|c| PreviewConflictDto {
                kind: conflict_kind_key(c.kind).to_string(),
                existing_name: c.existing_name.clone(),
            }),
        })
        .collect();

    // §3.1.9 (b): die Liste der Dateien, die auf Weg (b) aufgingen — damit
    // die Vorschau sie **vorher** nennen kann.
    let mut identity_file_paths: Vec<String> = plan
        .entries
        .iter()
        .filter_map(|e| e.identity_file.as_ref().map(|i| i.path.clone()))
        .collect();
    identity_file_paths.sort();
    identity_file_paths.dedup();

    ImportPreviewDto {
        groups: plan
            .groups
            .iter()
            .map(|g| PreviewGroupDto {
                name: g.name.clone(),
                parent: g.parent,
                source_path: g.source_path.clone(),
            })
            .collect(),
        entries,
        files: files
            .iter()
            .map(|f| PreviewFileDto {
                path: f.path.clone(),
                depth: f.depth,
                status: match f.status {
                    crate::ssh_config_import::FileStatus::Read => "read",
                    crate::ssh_config_import::FileStatus::NotSshConfig => "notSshConfig",
                    crate::ssh_config_import::FileStatus::Unreadable => "unreadable",
                    crate::ssh_config_import::FileStatus::AlreadyRead => "alreadyRead",
                }
                .to_string(),
            })
            .collect(),
        skipped: plan
            .skipped
            .iter()
            .map(|s| PreviewSkippedDto {
                file: s.file.clone(),
                line: s.line,
                directive: match &s.directive {
                    SkippedKind::Directive(d) => Some(d.clone()),
                    // §3.1.5: „unlesbare Zeile" — Zeilennummer und sonst
                    // nichts.
                    SkippedKind::UnreadableLine => None,
                },
                reason: reason_key(&s.reason),
                entry: s.entry.clone(),
            })
            .collect(),
        identity_file_paths,
    }
}

/// Vorschau (§3.1.7). Öffnet den Dateidialog **im Backend** — das Frontend
/// übergibt nie einen Pfad (Spec 0013/SEC-06, s. `AppState::
/// pending_ssh_config_import`). Liest ausschließlich `ssh_config`-Dateien;
/// eine Schlüsseldatei wird hier **nicht** angefasst (§5.1).
#[tauri::command]
pub async fn preview_ssh_config_import(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::state::AppState>,
    title: String,
) -> CommandResult<Option<ImportPreviewDto>> {
    use tauri_plugin_dialog::DialogExt;

    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(&title)
        .pick_file(move |path| {
            let _ = tx.send(path);
        });
    let Some(picked) = rx.await.ok().flatten() else {
        return Ok(None);
    };
    let path = picked.into_path()?;

    let read = crate::ssh_config_import::read_import(&path)
        .map_err(|abort| CommandError::from(abort.to_string()))?;

    // §4.1: Ohne den Bestand sind weder Konflikte noch `ProxyJump`-Ziele im
    // Bestand noch Schlagwort-Treffer bestimmbar.
    let servers = state.profile_store.list_servers().await?;
    let groups = state.profile_store.list_groups().await?;
    // §5.2a: die bestehenden Regeln, damit die Vorschau kennzeichnen kann,
    // welches importierte Schlagwort eine davon trifft.
    let rules: Vec<ssh_manager_core::filter::Rule> = state
        .policy_store
        .list_all()
        .await?
        .into_iter()
        .map(|r| r.into_rule())
        .collect();

    let plan = ssh_manager_core::profiles::ssh_config::build_plan(
        &read.sources,
        ssh_manager_core::profiles::ssh_config::Inventory {
            servers: &servers,
            groups: &groups,
            rules: &rules,
            local_server_id: crate::local_server::LOCAL_SERVER_ID,
        },
    );

    // Die `Include`-Befunde gehören mit in die Vorschau (§3.1.4).
    let mut plan = plan;
    plan.skipped.extend(read.include_issues.clone());

    let dto = build_preview_dto(&plan, &read.files, &servers);
    *state
        .pending_ssh_config_import
        .lock()
        .expect("pending import lock") = Some(PendingImport { plan });
    Ok(Some(dto))
}

/// Den bestätigten Plan ausführen (§3.1.11). Nimmt **nur Indizes** — der
/// Plan selbst kommt aus dem `AppState`, nicht vom Frontend (§5.1).
#[tauri::command]
pub async fn apply_ssh_config_import(
    state: tauri::State<'_, crate::state::AppState>,
    choices: Vec<EntryChoice>,
) -> CommandResult<ApplyOutcome> {
    // **Nehmen und leeren in einem Zug**, unter derselben Sperre: Vorher
    // wurde nur geklont und erst nach Erfolg geleert - zwei ueberlappende
    // Aufrufe sahen beide `Some` und haetten beide angelegt (Review-Runde 1).
    // `take` macht aus dem Kommentar "der Plan ist verbraucht" eine Tatsache.
    let pending = state
        .pending_ssh_config_import
        .lock()
        .expect("pending import lock")
        .take();
    let Some(pending) = pending else {
        return Err(CommandError::from(
            "Es liegt keine Import-Vorschau vor. Bitte die Datei erneut wählen.".to_string(),
        ));
    };

    let outcome = apply_import(
        &pending.plan,
        &choices,
        state.profile_store.as_ref(),
        state.credential_store.as_ref(),
        state.keychain,
        &DiskKeyFiles {
            reader: state.key_file_reader.as_ref(),
        },
    )
    .await?;
    Ok(outcome)
}

#[cfg(test)]
mod tests;
