//! DTOs: server.

use serde::{Deserialize, Serialize};

use credentials_keyring::{KeychainAvailability, KeychainUnavailableReason};
use ssh_manager_core::profiles::{
    AuthMethod, CredentialError, CredentialStore, GroupId, PostIngestPolicy, Server,
};
use ssh_manager_core::shared::ServerId;

use crate::credential_slots::sudo_password_credential_ref;

/// Spec 0084, §4 (Schnitt `dto` → `local_server::is_local`): hierher
/// verschoben, weil `dto` (Tauri-frei, zieht nach `app-logic`) diese
/// reine Funktion braucht, `local_server` selbst aber Tauri-gebunden
/// bleibt (`AppHandle`/`tauri-plugin-store` für Notizen/Tags des lokalen
/// Pseudo-Servers). `local_server` verwendet diese Konstante/Funktion
/// weiterhin, jetzt über diesen Pfad.
///
/// Spec 0032, Abschnitt 3: fest reservierte, konstante `ServerId` — die
/// Nil-UUID kann nie von `ServerId::new()` (UUID v4) erzeugt werden, daher
/// als Reservierung kollisionsfrei.
pub const LOCAL_SERVER_ID: ServerId = ServerId(uuid::Uuid::nil());

pub fn is_local(id: ServerId) -> bool {
    id == LOCAL_SERVER_ID
}

/// Sicht auf einen [`Server`] für Liste und Bearbeiten-Formular (Spec
/// 0007 Abschnitt 7 zunächst nur für die Liste eingeführt, Spec 0008
/// Abschnitt 4 erweitert sie um die für das Formular nötigen Felder,
/// inkl. `notes` — analog zu [`GroupDto`]s `notes`-Feld: ohne dieses
/// Feld bräuchte das Server-Formular einen eigenen Befehl nur für die
/// Notiz-Vorbefüllung, obwohl `get_server`/`list_servers` ohnehin schon
/// die volle `Server`-Struktur lesen. **Kein** Secret-Inhalt — nur
/// [`AuthMethodKind`], welche Methode aktiv ist, nie ein `CredentialRef`
/// oder gar das Secret selbst (Spec 0008 Abschnitt 4: "ServerDto ...
/// enthält keine Secret-Felder").
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerDto {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub group_id: Option<String>,
    pub tags: Vec<String>,
    pub auth_kind: AuthMethodKind,
    /// Spec 0076, B-4: **wo** die Schlüsseldatei liegt — `None` bei jeder
    /// anderen Anmeldeart.
    ///
    /// Das einzige Auth-Detail, das dieses DTO trägt, und die Begründung
    /// steht direkt darüber: `auth_kind` ist bewusst inhaltslos, weil dort
    /// nie ein Geheimnis hinaus soll. **Ein Dateipfad ist kein Geheimnis**
    /// — er steht ohnehin im Klartext in der Datenbank —, und ohne ihn
    /// lässt sich B-4 nicht umsetzen: Liste und Details sollen zeigen,
    /// dass die Anmeldung an einer Datei hängt, und an welcher.
    ///
    /// Was hier weiterhin **nicht** hinausgeht: der Dateiinhalt, die
    /// Passphrase und deren `CredentialRef`.
    pub identity_file_path: Option<String>,
    pub jump_host: Option<String>,
    pub notes: String,
    /// Spec 0018, Abschnitt 4: ob für diesen Server ein Sudo-Passwort im
    /// `CredentialStore` hinterlegt ist — **nicht** aus `Server` selbst
    /// ableitbar (kein DB-Feld, s. `server_credentials`-Doc-
    /// Kommentar), deshalb kein `From<&Server>`, sondern
    /// [`ServerDto::from_server`] mit explizitem `CredentialStore`-Zugriff.
    pub has_sudo_password: bool,
    /// Spec 0071, A14/I4: `true`, wenn der Schlüsselbund **nicht sagen
    /// konnte**, ob ein Sudo-Passwort hinterlegt ist (`CredentialError::
    /// Backend`). Vorher lieferte `.is_ok()` in diesem Fall `false` — die
    /// Oberfläche behauptete also "kein Sudo-Passwort hinterlegt", obwohl
    /// sie es schlicht nicht wusste. "Unbekannt" ist nicht "nein".
    ///
    /// `has_sudo_password` ist dann ebenfalls `false`; die Oberfläche muss
    /// dieses Feld zuerst prüfen und einen neutralen Zustand anzeigen,
    /// statt eine Aussage zu treffen.
    pub sudo_password_unknown: bool,
    /// Spec 0032, Abschnitt 3: `true` genau für den lokalen Pseudo-Server
    /// ([`LOCAL_SERVER_ID`]) — steuert im Frontend, ob
    /// Host/Port/Nutzername/Auth/Jump-Host/Löschen/Verbindungstest
    /// ausgeblendet werden.
    pub is_local: bool,
    /// Spec 0039, Abschnitt 5.1.
    pub post_ingest_policy: PostIngestPolicy,
    /// Spec 0039, Abschnitt 5.2. Das Frontend gated die zugehörige
    /// Checkbox zusätzlich anhand der app-weiten Zweitmeinungs-Einstellung
    /// (`riskSettings.ts`), unabhängig von diesem Feld.
    pub ai_injection_check_enabled: bool,
    /// Spec 0067, A2: `None` = automatisch.
    pub sftp_server_path: Option<String>,
    /// Spec 0102: `None` = Home des Login-Nutzers.
    pub start_directory: Option<String>,
    /// Spec 0103: the server was moved into "Previous data" by a backup
    /// import. Not part of `Server` (no effect on connecting, rules or the
    /// filter engine); the list commands fill it from the marker store.
    pub is_copy: bool,
}

impl ServerDto {
    pub fn from_server(server: &Server, credential_store: &dyn CredentialStore) -> Self {
        // Spec 0071, A14/X4: drei Ausgänge statt zwei. Der Unterschied
        // zwischen "es gibt keinen Eintrag" und "der Schlüsselbund konnte
        // nicht antworten" steckt bereits im Fehlertyp — es braucht dafür
        // keinen zusätzlichen Blick auf den Schlüsselbund-Zustand (A16).
        let (has_sudo_password, sudo_password_unknown) =
            match credential_store.get(&sudo_password_credential_ref(server.id)) {
                Ok(_) => (true, false),
                Err(CredentialError::NotFound(_)) => (false, false),
                Err(CredentialError::Backend(_)) => (false, true),
            };
        Self {
            id: server.id.0.to_string(),
            name: server.name.clone(),
            host: server.host.clone(),
            port: server.port,
            username: server.username.clone(),
            group_id: server.group_id.map(|g| g.0.to_string()),
            tags: server.tags.clone(),
            auth_kind: AuthMethodKind::from(&server.auth),
            identity_file_path: match &server.auth {
                AuthMethod::IdentityFile { path, .. } => Some(path.clone()),
                _ => None,
            },
            jump_host: server.jump_host.map(|j| j.0.to_string()),
            notes: server.notes.clone(),
            has_sudo_password,
            sudo_password_unknown,
            is_local: is_local(server.id),
            post_ingest_policy: server.post_ingest_policy,
            ai_injection_check_enabled: server.ai_injection_check_enabled,
            sftp_server_path: server.sftp_server_path.clone(),
            start_directory: server.start_directory.clone(),
            is_copy: false,
        }
    }
}

/// Spec 0076, B-3/C-7: der Vorab-Befund über eine Schlüsseldatei.
///
/// **Kein Feld für den Schlüssel.** Der Befund entsteht über
/// [`ssh_manager_core::ssh::KeyFileReader::inspect`], das per Konstruktion
/// kein Schlüsselmaterial herausgibt (§4.2) — dieses DTO hat deshalb gar
/// keine Stelle, an der welches hinausgehen könnte (§5.2). Der Pfad ist
/// ebenfalls nicht enthalten: Das Frontend hat ihn gerade selbst
/// geschickt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredHostKeyDto {
    /// SSH algorithm name, or `unknown`.
    pub algorithm: String,
    /// `SHA256:<base64>`; the raw key never leaves the backend.
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyFileFactsDto {
    pub exists: bool,
    /// Nur Unix; auf Windows immer `false`, wie bei OpenSSH selbst (A-4).
    pub permissions_too_open: bool,
    pub valid_key: bool,
    pub encrypted: bool,
    /// `None`, wenn die Datei grundsätzlich in Frage kommt. Sonst der
    /// Grund, warum nicht — mit stabilem Code fürs Übersetzen (Spec 0024,
    /// Abschnitt 5) und dem Text als Rückfallebene.
    pub problem: Option<KeyFileProblemDto>,
}

/// Der Grund aus [`KeyFileFactsDto::problem`]. Trägt **nie** Dateiinhalt —
/// die Meldungen stammen aus [`ssh_manager_core::ssh::KeyFileError`], die
/// genau das zusichert (§5.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyFileProblemDto {
    pub code: String,
    pub message: String,
}

impl From<ssh_manager_core::ssh::KeyFileFacts> for KeyFileFactsDto {
    fn from(facts: ssh_manager_core::ssh::KeyFileFacts) -> Self {
        Self {
            exists: facts.exists,
            permissions_too_open: facts.permissions_too_open,
            valid_key: facts.valid_key,
            encrypted: facts.encrypted,
            problem: facts.problem.map(|err| KeyFileProblemDto {
                code: err.code().to_string(),
                message: err.to_string(),
            }),
        }
    }
}

/// Spec 0071, A15: der Schlüsselbund-Zustand für die Diagnose-Ansicht.
///
/// Bewusst nur ein Flag und eine Aufzählung — **kein** Fehlertext, keine
/// D-Bus-Adresse, kein Pfad (I1/A10). Die anzeigbaren Texte liegen im
/// Frontend-Übersetzungskatalog, nicht hier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeychainStatusDto {
    pub available: bool,
    /// `None`, wenn `available` — sonst der klassifizierte Grund.
    pub reason: Option<KeychainUnavailableReasonDto>,
}

/// Serialisierbare Fassung von
/// [`credentials_keyring::KeychainUnavailableReason`]. Eigener Typ statt
/// `Serialize` auf dem Core-Enum: `credentials-keyring` soll `serde` nicht
/// kennen müssen (dieselbe Trennung wie bei allen anderen DTOs hier).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KeychainUnavailableReasonDto {
    NoSessionBus,
    NoSecretServiceProvider,
    Locked,
    Unknown,
}

impl From<KeychainAvailability> for KeychainStatusDto {
    fn from(availability: KeychainAvailability) -> Self {
        match availability.unavailable_reason() {
            None => Self {
                available: true,
                reason: None,
            },
            Some(reason) => Self {
                available: false,
                reason: Some(match reason {
                    KeychainUnavailableReason::NoSessionBus => {
                        KeychainUnavailableReasonDto::NoSessionBus
                    }
                    KeychainUnavailableReason::NoSecretServiceProvider => {
                        KeychainUnavailableReasonDto::NoSecretServiceProvider
                    }
                    KeychainUnavailableReason::Locked => KeychainUnavailableReasonDto::Locked,
                    // Android has no dedicated settings text yet; the generic
                    // "cause unknown" line applies there.
                    KeychainUnavailableReason::NoKeystore | KeychainUnavailableReason::Unknown => {
                        KeychainUnavailableReasonDto::Unknown
                    }
                }),
            },
        }
    }
}

/// Welche [`AuthMethod`]-Variante aktiv ist, ohne deren Inhalt (Spec 0008
/// Abschnitt 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethodKind {
    Password,
    /// Issue #632: Passwortmethode ohne gespeichertes Passwort.
    NoPassword,
    PrivateKey,
    Agent,
    Certificate,
    /// Spec 0076, A-1. Wie die übrigen Varianten **ohne** Inhalt — der
    /// Pfad reist in einem eigenen Feld des [`ServerDto`] (B-4), damit
    /// dieses Enum weiterhin nur „welche Methode" sagt.
    IdentityFile,
}

impl From<&AuthMethod> for AuthMethodKind {
    fn from(auth: &AuthMethod) -> Self {
        match auth {
            AuthMethod::Password { .. } => AuthMethodKind::Password,
            AuthMethod::NoPassword => AuthMethodKind::NoPassword,
            AuthMethod::PrivateKey { .. } => AuthMethodKind::PrivateKey,
            AuthMethod::Agent => AuthMethodKind::Agent,
            AuthMethod::Certificate { .. } => AuthMethodKind::Certificate,
            AuthMethod::IdentityFile { .. } => AuthMethodKind::IdentityFile,
        }
    }
}

/// Vorschau bzw. Ergebnis von `delete_server` (Spec 0046, Fund 1 — analog
/// zu [`DeleteGroupResult`]). `server` trägt bereits `authKind`/
/// `hasSudoPassword` (s. [`ServerDto::from_server`]), das Frontend kann
/// daraus ableiten, welche Keychain-Secrets beim Löschen entfernt würden,
/// ohne dass diese DTO die Secrets selbst benennen muss.
/// `servers_losing_jump_host` sind die anderen Server, deren `jump_host`
/// beim tatsächlichen Löschen still auf `NULL` gesetzt würde.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteServerResult {
    pub server: ServerDto,
    pub servers_losing_jump_host: Vec<ServerDto>,
    pub executed: bool,
    /// Spec 0071, A17: Secrets, die beim Löschen **nicht** aus dem
    /// Schlüsselbund entfernt werden konnten (nicht verfügbarer oder
    /// gesperrter Schlüsselbund). Der Server ist trotzdem gelöscht — das
    /// ist die bewusste Entscheidung aus A17, damit niemand auf einem
    /// unlöschbaren Server sitzen bleibt.
    ///
    /// Die Einträge sind danach **verwaist**: Sie tragen die ID eines
    /// Servers, den es nicht mehr gibt. Deshalb stehen hier die
    /// `CredentialRef`-Strings selbst — der Nutzer braucht sie, um die
    /// Einträge im Schlüsselbund-Verwaltungsprogramm wiederzufinden. Das
    /// ist **kein** Secret (s. `CredentialRef`-Doc-Kommentar), nur der
    /// Account-Name innerhalb des Service „Smart SSH".
    ///
    /// Leer im Normalfall und immer leer bei `executed: false` (dann wurde
    /// nichts gelöscht).
    pub secrets_left_behind: Vec<String>,
}

/// Issue #100: warum ein Server nicht nutzbar ist — als stabiler Code, den
/// das Frontend übersetzt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnusableReasonDto {
    /// Von einer neueren Version gespeichert; diese kennt die Anmeldeart
    /// nicht.
    UnknownAuthMethod,
    /// Die gespeicherte Anmeldeart ist beschädigt.
    UnreadableAuthMethod,
}

impl From<ssh_manager_core::profiles::UnusableReason> for UnusableReasonDto {
    fn from(reason: ssh_manager_core::profiles::UnusableReason) -> Self {
        use ssh_manager_core::profiles::UnusableReason;
        match reason {
            UnusableReason::UnknownAuthMethod => Self::UnknownAuthMethod,
            UnusableReason::UnreadableAuthMethod => Self::UnreadableAuthMethod,
        }
    }
}

/// Issue #100: ein Server, dessen Anmeldeart diese Version nicht lesen
/// kann. Nur Anzeige und Löschen — keine Anmeldeart, kein Port, kein
/// Benutzer, damit das Frontend ihn gar nicht wie einen nutzbaren Server
/// behandeln kann.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnusableServerDto {
    pub id: ServerId,
    pub name: String,
    pub host: String,
    pub group_id: Option<GroupId>,
    pub reason: UnusableReasonDto,
}

impl From<&ssh_manager_core::profiles::UnusableServer> for UnusableServerDto {
    fn from(server: &ssh_manager_core::profiles::UnusableServer) -> Self {
        Self {
            id: server.id,
            name: server.name.clone(),
            host: server.host.clone(),
            group_id: server.group_id,
            reason: server.reason.into(),
        }
    }
}

/// Issue #100: Vorschau bzw. Ergebnis von `delete_unusable_server` — wie
/// [`DeleteServerResult`], nur mit dem nicht nutzbaren Eintrag. Welche
/// Secrets er hatte, ist unbekannt (die Anmeldeart ist ja unlesbar);
/// gelöscht werden deshalb alle Slots seines festen Schemas
/// `server:<id>:<slot>`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteUnusableServerResult {
    pub server: UnusableServerDto,
    pub servers_losing_jump_host: Vec<ServerDto>,
    pub executed: bool,
    /// Wie [`DeleteServerResult::secrets_left_behind`]: was beim Löschen
    /// nicht entfernt werden konnte. Leer bei `executed: false`.
    pub secrets_left_behind: Vec<String>,
}

/// Eingabe für `create_server`/`update_server`/`test_connection` (Spec
/// 0008, Abschnitt 4). `group_id`/`jump_host` direkt als `GroupId`/
/// `ServerId` statt `String` (beides `Uuid`-Newtypes, kein Parsen nötig).
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInput {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub group_id: Option<GroupId>,
    pub tags: Vec<String>,
    pub auth: AuthMethodInput,
    pub jump_host: Option<ServerId>,
    /// Spec 0018, Abschnitt 4: "leer/fehlend = unverändert" bei
    /// `update_server` (s. `server_credentials::resolve_sudo_password`),
    /// "nicht gesetzt" bei `create_server`. Explizites Entfernen eines
    /// bereits gesetzten Werts läuft über den eigenen
    /// `clear_server_sudo_password`-Befehl, nicht über dieses Feld.
    pub sudo_password: Option<String>,
    /// Spec 0039, Abschnitt 5.1. `#[serde(default)]`: fehlt das Feld (z. B.
    /// ein älterer Frontend-Build), gilt derselbe Default wie beim
    /// Migrations-Spaltendefault (`Balanced`), kein harter Fehler.
    #[serde(default)]
    pub post_ingest_policy: PostIngestPolicy,
    /// Spec 0039, Abschnitt 5.2. `#[serde(default)]`: fehlendes Feld ->
    /// `false`, derselbe Default wie der Migrations-Spaltendefault.
    #[serde(default)]
    pub ai_injection_check_enabled: bool,
    /// Spec 0067, A2: Override für den `sftp-server`-Pfad im erhöhten
    /// Dateibrowser. Leer/fehlend = automatisch (s.
    /// [`normalize_sftp_server_path`]).
    #[serde(default)]
    pub sftp_server_path: Option<String>,
    /// Spec 0102: optionales Startverzeichnis. Leer/fehlend = nicht gesetzt
    /// (s. [`normalize_start_directory_input`]).
    #[serde(default)]
    pub start_directory: Option<String>,
}

/// Spec 0008, Abschnitt 4. `#[serde(tag = "kind", rename_all =
/// "camelCase")]` (wie bei [`ActionUserDecision`]/[`HostKeyUserDecision`])
/// statt Serdes Standard-Außen-Tagging — ergibt z. B. `{"kind":
/// "privateKey", "keyContent": "...", "passphrase": null}`, die
/// natürliche Form für das TypeScript-Frontend, dieses Objekt selbst zu
/// bauen.
#[derive(Clone, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AuthMethodInput {
    /// Leeres/`None`-Feld heißt hier: bei einem bestehenden Passwort
    /// unverändert, sonst (Neuanlage, Methodenwechsel) "kein Passwort"
    /// (Issue #632). Ein Wert, der nach dem Trimmen (Spec 0073) leer ist,
    /// zählt wie `None`.
    Password {
        value: Option<String>,
    },
    /// Issue #632: ausdrücklich "kein Passwort" — entfernt auch ein
    /// gespeichertes. Eigene Variante, damit "entfernen" nie mit "leer =
    /// unverändert" verwechselt werden kann.
    NoPassword,
    PrivateKey {
        key_content: Option<String>,
        passphrase: Option<String>,
    },
    Agent,
    Certificate {
        cert_content: Option<String>,
        key_content: Option<String>,
    },
    /// Spec 0076, A-1/B-2. Anders als bei den übrigen Varianten ist `path`
    /// **kein** Secret und **nicht** optional: „leer = unverändert lassen"
    /// gilt für Schlüsselbund-Slots, nicht für ein Klartextfeld, das
    /// ohnehin vorbefüllt aus dem `ServerDto` zurückkommt (B-4).
    ///
    /// `passphrase` verhält sich dagegen genau wie bei
    /// [`AuthMethodInput::PrivateKey`]: leer bedeutet unverändert (A-5).
    IdentityFile {
        path: String,
        passphrase: Option<String>,
    },
}

/// Spec 0008, Abschnitt 7. Weicht in zwei Punkten von der Spec-Skizze ab:
///
/// 1. `NetworkError(String)` wurde zu `NetworkError { message: String }`
///    — ein Tupel-Variant lässt sich unter internem Tagging
///    (`#[serde(tag = "kind")]`, s. o.) nicht darstellen (serde verlangt
///    dafür Struct-artige Varianten).
/// 2. `HostKeyUnknown`/`HostKeyMismatch` tragen zusätzlich `host`/`port`/
///    `raw_key` — die Spec-Skizze nennt dort nur die Fingerprints. Ohne
///    die Rohdaten hätte das Frontend keine Möglichkeit, nach einer
///    Nutzerbestätigung `trust_host_key` (neuer, in der Spec nicht
///    vorgesehener Befehl, s. Doc-Kommentar dort) aufzurufen — die Spec
///    selbst verlangt aber ausdrücklich, dass "bei Zustimmung `trust()`
///    aufgerufen" werden kann (Abschnitt 7).
#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TestConnectionResult {
    Success,
    AuthenticationFailed,
    HostKeyUnknown {
        host: String,
        port: u16,
        raw_key: Vec<u8>,
        fingerprint: String,
        /// Algorithm of the offered key; `None` if undeterminable.
        key_type: Option<String>,
    },
    HostKeyMismatch {
        host: String,
        port: u16,
        raw_key: Vec<u8>,
        expected_fingerprint: String,
        actual_fingerprint: String,
        /// Algorithm of the offered key; `None` if undeterminable.
        key_type: Option<String>,
    },
    NetworkError {
        message: String,
        /// Spec 0069, Teil A4/E3: additiv, optional — ein altes Frontend
        /// ignoriert das Feld, ein unbekannter Code fällt im Frontend auf
        /// `message` zurück. Gesetzt aus `SshError::code()`.
        code: Option<&'static str>,
    },
    Timeout,
}

/// Issue #51: Ergebnis von `test_connection` samt Schritt-Protokoll.
///
/// `result` wird flach eingebettet: Die JSON-Form bleibt die von
/// [`TestConnectionResult`] (`{"kind": …, …}`), `steps` kommt als weiteres
/// Feld hinzu — ein Frontend, das `steps` nicht kennt, liest das Ergebnis
/// unverändert.
///
/// Die Schritte sind nur für die Anzeige bestimmt und werden nirgends
/// geloggt oder exportiert.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestConnectionReport {
    #[serde(flatten)]
    pub result: TestConnectionResult,
    pub steps: Vec<ssh_manager_core::ssh::ConnectStepRecord>,
}

// --- Spec 0009: Filter-Regel-Verwaltung ---------------------------------
