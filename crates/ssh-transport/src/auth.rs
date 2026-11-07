use std::sync::Arc;

use russh::client::{self, AuthResult};
use russh::keys::{Certificate, PrivateKey, PrivateKeyWithHashAlg};
use secrecy::{ExposeSecret, SecretString};
use ssh_manager_core::profiles::AuthMethod;
use ssh_manager_core::profiles::CredentialStore;
use ssh_manager_core::ssh::{
    resolve_auth, AuthMethodKind, AuthPromptKind, AuthPromptRequest, AuthPrompter, ConnectLog,
    ConnectStep, Hop, KeyFileReader, PromptCancelled, ResolvedAuth, SshError, StepId,
    AUTH_PROMPT_TIMEOUT,
};

use crate::auth_pause::AuthPause;

use crate::error::map_russh_error;
use crate::handler::ClientHandler;

/// Issue #1184: at most this many `keyboard-interactive` info requests per hop.
const MAX_KI_ROUNDS: u32 = 10;
/// Issue #1184: at most this many partial-success follow-up steps per hop.
const MAX_PARTIAL_STEPS: u32 = 3;
const KI: &str = "keyboard-interactive";

/// What [`authenticate`] needs to ask the user and to pause the time limit.
pub(crate) struct AuthPrompting<'a> {
    pub(crate) prompter: &'a dyn AuthPrompter,
    pub(crate) pause: &'a AuthPause,
}

/// The follow-up step after a partial success.
enum Next {
    KeyboardInteractive { stored: bool },
    Password,
}

/// Authentifiziert `handle` für den gegebenen `hop` (Spec 0005, Teil 2 Punkt
/// 3 der Aufgabenstellung): löst `hop.auth` über `credentials` auf (reine
/// `core`-Logik, s. `ssh_manager_core::ssh::resolve_auth`) und übersetzt das
/// Ergebnis in den passenden `russh`-Auth-Aufruf.
///
/// Spec 0076, §4.2: `key_files` reist neben `credentials` mit.
///
/// Issue #51: `steps` zeichnet jeden Versuch als Schritt auf.
///
/// Issue #1184: Die konfigurierte Methode wird wie bisher zuerst versucht.
/// Weitere Schritte gibt es nur nach Teilerfolg des Servers
/// (`partial_success`, höchstens [`MAX_PARTIAL_STEPS`]) und, für ein
/// `Password`-Profil, wenn der Server nur `keyboard-interactive` anbietet.
/// Antworten holt `prompting.prompter`; währenddessen steht die Zeitgrenze.
pub(crate) async fn authenticate(
    handle: &mut client::Handle<ClientHandler>,
    hop: &Hop,
    credentials: &(dyn CredentialStore + Send + Sync),
    key_files: &(dyn KeyFileReader + Send + Sync),
    steps: &AuthSteps<'_>,
    prompting: &AuthPrompting<'_>,
) -> Result<(), SshError> {
    // Issue #632: "kein Passwort" = erst `none`, dann `password` mit leerem
    // Wert; je Versuch ein eigener Protokollschritt.
    if matches!(hop.auth, AuthMethod::NoPassword) {
        return authenticate_without_password(handle, hop, steps).await;
    }
    let step = steps.log.start(
        steps.hop_index,
        steps.hop,
        auth_step(AuthMethodKind::from(&hop.auth), None, Vec::new()),
    );
    let resolved = resolve_auth(&hop.auth, credentials, key_files).map_err(|e| name_hop(e, hop))?;

    // The stored password of a `Password` profile, kept only for the one
    // `keyboard-interactive` fallback below.
    let mut stored_password: Option<SecretString> = None;
    let mut tried: &'static str = "publickey";
    let mut result = match resolved {
        ResolvedAuth::Password(secret) => {
            tried = "password";
            let r = handle
                .authenticate_password(hop.username.clone(), secret.expose_secret().to_string())
                .await
                .map_err(map_russh_error)?;
            stored_password = Some(secret);
            r
        }
        // Von `authenticate_without_password` oben abgefangen; hier nie
        // erreichbar, aber kein stilles Durchwinken.
        ResolvedAuth::NoPassword => return Err(SshError::AuthenticationFailed),
        ResolvedAuth::PrivateKey { key, passphrase } => {
            let private_key =
                load_private_key(&key, passphrase.as_ref()).map_err(|e| name_hop(e, hop))?;
            let hash_alg = handle
                .best_supported_rsa_hash()
                .await
                .unwrap_or(None)
                .flatten();
            // Issue #1173: an RSA key signing with SHA-1 (`ssh-rsa`).
            if private_key.algorithm().is_rsa() && hash_alg.is_none() {
                steps.log.warn_outdated(
                    steps.hop_index,
                    steps.hop,
                    vec![crate::algorithms::LEGACY_HOST_KEY.to_string()],
                );
            }
            let key_with_hash = PrivateKeyWithHashAlg::new(Arc::new(private_key), hash_alg);
            handle
                .authenticate_publickey(hop.username.clone(), key_with_hash)
                .await
                .map_err(map_russh_error)?
        }
        ResolvedAuth::Agent => authenticate_via_agent(handle, &hop.username).await?,
        ResolvedAuth::Certificate { cert, key } => {
            let private_key = load_private_key(&key, None).map_err(|e| name_hop(e, hop))?;
            let certificate = Certificate::from_openssh(cert.expose_secret()).map_err(|e| {
                name_hop(
                    SshError::CredentialResolutionFailed(format!("Zertifikat ungültig: {e}")),
                    hop,
                )
            })?;
            handle
                .authenticate_openssh_cert(hop.username.clone(), Arc::new(private_key), certificate)
                .await
                .map_err(map_russh_error)?
        }
    };
    let mut last_tried = tried;
    let mut ki_fallback_done = false;
    record_result(steps, step, &result);

    let mut partial_steps = 0u32;
    loop {
        let AuthResult::Failure {
            remaining_methods,
            partial_success,
        } = &result
        else {
            return Ok(());
        };
        let remaining: Vec<String> = remaining_methods
            .iter()
            .map(|m| <&'static str>::from(m).to_string())
            .collect();
        let has = |m: &str| remaining.iter().any(|r| r == m);
        let next = if *partial_success {
            partial_steps += 1;
            if partial_steps > MAX_PARTIAL_STEPS {
                return Err(SshError::AuthenticationFailed);
            }
            // Order: publickey, keyboard-interactive, password. `publickey`
            // is never a follow-up here: it needs the profile's key, which
            // is always the first attempt and cannot be repeated.
            if has(KI) {
                Some(Next::KeyboardInteractive { stored: false })
            } else if has("password") {
                Some(Next::Password)
            } else {
                None
            }
        } else if stored_password.is_some() && has(KI) && !has("password") && !ki_fallback_done {
            ki_fallback_done = true;
            Some(Next::KeyboardInteractive { stored: true })
        } else {
            None
        };
        let Some(next) = next else {
            return Err(failure_error(&remaining, Some(last_tried)));
        };
        match next {
            Next::KeyboardInteractive { stored } => {
                last_tried = KI;
                let stored = if stored {
                    stored_password.as_ref()
                } else {
                    None
                };
                result =
                    keyboard_interactive(handle, hop, steps, prompting, stored, &remaining).await?;
            }
            Next::Password => {
                last_tried = "password";
                result = prompted_password(handle, hop, steps, prompting, &remaining).await?;
            }
        }
    }
}

fn auth_step(method: AuthMethodKind, round: Option<u32>, prompts: Vec<String>) -> ConnectStep {
    ConnectStep::Authentication {
        method,
        remaining_methods: Vec::new(),
        partial_success: false,
        round,
        prompts,
    }
}

/// The error for a login that cannot continue: when the server no longer
/// offers the method just tried but wants others (which the client cannot
/// supply here), the message names them (Issue #1184).
fn failure_error(remaining: &[String], last_tried: Option<&str>) -> SshError {
    let methods: Vec<String> = remaining
        .iter()
        .filter(|m| m.as_str() != "none")
        .cloned()
        .collect();
    let offers_tried = last_tried.is_some_and(|t| methods.iter().any(|m| m == t));
    if methods.is_empty() || offers_tried {
        SshError::AuthenticationFailed
    } else {
        SshError::AuthenticationMethodRequired { methods }
    }
}

/// Asks the prompter with the hop's time limit paused. The prompter gives up
/// after [`AUTH_PROMPT_TIMEOUT`] at the latest.
async fn ask(
    prompting: &AuthPrompting<'_>,
    request: AuthPromptRequest,
) -> Result<Vec<SecretString>, PromptCancelled> {
    ask_within(prompting, request, AUTH_PROMPT_TIMEOUT).await
}

async fn ask_within(
    prompting: &AuthPrompting<'_>,
    request: AuthPromptRequest,
    limit: std::time::Duration,
) -> Result<Vec<SecretString>, PromptCancelled> {
    let _paused = prompting.pause.pause();
    match tokio::time::timeout(limit, prompting.prompter.prompt(request)).await {
        Ok(answers) => answers,
        Err(_elapsed) => Err(PromptCancelled),
    }
}

/// Password step of a multi-step login: the password comes from the
/// prompter, never from storage.
async fn prompted_password(
    handle: &mut client::Handle<ClientHandler>,
    hop: &Hop,
    steps: &AuthSteps<'_>,
    prompting: &AuthPrompting<'_>,
    server_methods: &[String],
) -> Result<AuthResult, SshError> {
    let step = steps.log.start(
        steps.hop_index,
        steps.hop,
        auth_step(AuthMethodKind::Password, None, Vec::new()),
    );
    let answers = ask(prompting, AuthPromptRequest::password(steps.hop))
        .await
        .map_err(|PromptCancelled| SshError::AuthenticationMethodRequired {
            methods: server_methods.to_vec(),
        })?;
    let [password] = answers.as_slice() else {
        return Err(SshError::AuthenticationFailed);
    };
    let result = handle
        .authenticate_password(hop.username.clone(), password.expose_secret().to_string())
        .await
        .map_err(map_russh_error)?;
    drop(answers);
    record_result(steps, step, &result);
    Ok(result)
}

/// `keyboard-interactive` login (Issue #1184). `stored` is the password of a
/// `Password` profile: it answers exactly one info request with a single
/// prompt with echo off; everything else goes to the prompter. Every round
/// is its own log step with the prompt texts, never the answers.
async fn keyboard_interactive(
    handle: &mut client::Handle<ClientHandler>,
    hop: &Hop,
    steps: &AuthSteps<'_>,
    prompting: &AuthPrompting<'_>,
    stored: Option<&SecretString>,
    server_methods: &[String],
) -> Result<AuthResult, SshError> {
    use russh::client::KeyboardInteractiveAuthResponse as Response;

    let mut response = handle
        .authenticate_keyboard_interactive_start(hop.username.clone(), None)
        .await
        .map_err(map_russh_error)?;
    let mut stored = stored;
    let mut round = 0u32;
    let mut previous: Option<StepId> = None;
    loop {
        match response {
            Response::Success => {
                if let Some(step) = previous {
                    steps.log.succeed(step);
                }
                return Ok(AuthResult::Success);
            }
            Response::Failure {
                remaining_methods,
                partial_success,
            } => {
                let result = AuthResult::Failure {
                    remaining_methods,
                    partial_success,
                };
                if let Some(step) = previous {
                    record_result(steps, step, &result);
                }
                return Ok(result);
            }
            Response::InfoRequest {
                name,
                instructions,
                prompts,
            } => {
                if let Some(step) = previous.take() {
                    steps.log.succeed(step);
                }
                round += 1;
                if round > MAX_KI_ROUNDS {
                    return Err(SshError::AuthenticationFailed);
                }
                let pairs: Vec<(String, bool)> =
                    prompts.iter().map(|p| (p.prompt.clone(), p.echo)).collect();
                let Some(request) = AuthPromptRequest::keyboard_interactive(
                    steps.hop,
                    &name,
                    &instructions,
                    &pairs,
                ) else {
                    return Err(SshError::AuthenticationFailed);
                };
                let texts = match &request.kind {
                    AuthPromptKind::KeyboardInteractive { prompts, .. } => {
                        prompts.iter().map(|p| p.text.clone()).collect()
                    }
                    AuthPromptKind::Password => Vec::new(),
                };
                previous = Some(steps.log.start(
                    steps.hop_index,
                    steps.hop,
                    auth_step(AuthMethodKind::KeyboardInteractive, Some(round), texts),
                ));

                let answers: Vec<SecretString> = if prompts.is_empty() {
                    Vec::new()
                } else if let (Some(password), [only]) = (stored, prompts.as_slice()) {
                    if only.echo {
                        ask_user(prompting, request, prompts.len(), server_methods).await?
                    } else {
                        stored = None;
                        vec![SecretString::from(password.expose_secret().to_string())]
                    }
                } else {
                    ask_user(prompting, request, prompts.len(), server_methods).await?
                };
                let wire: Vec<String> = answers
                    .iter()
                    .map(|a| a.expose_secret().to_string())
                    .collect();
                drop(answers);
                response = handle
                    .authenticate_keyboard_interactive_respond(wire)
                    .await
                    .map_err(map_russh_error)?;
            }
        }
    }
}

async fn ask_user(
    prompting: &AuthPrompting<'_>,
    request: AuthPromptRequest,
    expected: usize,
    server_methods: &[String],
) -> Result<Vec<SecretString>, SshError> {
    let answers = ask(prompting, request).await.map_err(|PromptCancelled| {
        SshError::AuthenticationMethodRequired {
            methods: server_methods.to_vec(),
        }
    })?;
    if answers.len() == expected {
        Ok(answers)
    } else {
        Err(SshError::AuthenticationFailed)
    }
}

/// Trägt bei Ablehnung die noch angebotenen Methoden ein und schließt den
/// Schritt (Issue #1184: auch den abgelehnten, bevor ein weiterer beginnt).
fn record_result(steps: &AuthSteps<'_>, step: StepId, result: &AuthResult) {
    if let AuthResult::Failure {
        remaining_methods: remaining,
        partial_success: partial,
    } = result
    {
        steps.log.update(step, |s| {
            if let ConnectStep::Authentication {
                remaining_methods,
                partial_success,
                ..
            } = s
            {
                *remaining_methods = remaining
                    .iter()
                    .map(|m| <&'static str>::from(m).to_string())
                    .collect();
                *partial_success = *partial;
            }
        });
    }
    if result.success() {
        steps.log.succeed(step);
    } else {
        steps.log.fail(step, SshError::AuthenticationFailed.code());
    }
}

/// Wie [`record_result`], für Pfade ohne Folgeschritt.
fn finish_attempt(
    steps: &AuthSteps<'_>,
    step: StepId,
    result: &AuthResult,
) -> Result<(), SshError> {
    record_result(steps, step, result);
    if result.success() {
        Ok(())
    } else {
        Err(SshError::AuthenticationFailed)
    }
}

/// Issue #632: Anmeldung für [`AuthMethod::NoPassword`]. Erst SSH-Methode
/// `none`; lehnt der Server ab, `password` mit leerem Wert; lehnt er auch das
/// ab, der übliche Anmeldefehler. Beide Versuche stehen im Verbindungs-
/// protokoll. Erst nach einem abgelehnten `none` wird ein leeres Passwort
/// gesendet, und nur für diese Methode — jede andere Methode sendet nie ein
/// leeres Secret.
async fn authenticate_without_password(
    handle: &mut client::Handle<ClientHandler>,
    hop: &Hop,
    steps: &AuthSteps<'_>,
) -> Result<(), SshError> {
    let attempt = |method: AuthMethodKind| {
        steps.log.start(
            steps.hop_index,
            steps.hop,
            auth_step(method, None, Vec::new()),
        )
    };

    let none_step = attempt(AuthMethodKind::None);
    let none_result = handle
        .authenticate_none(hop.username.clone())
        .await
        .map_err(map_russh_error)?;
    if finish_attempt(steps, none_step, &none_result).is_ok() {
        return Ok(());
    }
    // `finish_attempt` hat den abgelehnten `none`-Schritt schon geschlossen.

    let password_step = attempt(AuthMethodKind::Password);
    let password_result = handle
        .authenticate_password(hop.username.clone(), String::new())
        .await
        .map_err(map_russh_error)?;
    finish_attempt(steps, password_step, &password_result)
}

/// Wohin [`authenticate`] seinen Schritt schreibt (Issue #51).
pub(crate) struct AuthSteps<'a> {
    log: &'a ConnectLog,
    hop_index: usize,
    hop: &'a str,
}

impl<'a> AuthSteps<'a> {
    pub(crate) fn new(log: &'a ConnectLog, hop_index: usize, hop: &'a str) -> Self {
        Self {
            log,
            hop_index,
            hop,
        }
    }
}

/// Spec 0076, A-8: **Die Meldung muss sagen, welcher Hop gescheitert ist.**
/// `authenticate` läuft je Hop der Verbindungskette (`connect::connect`).
///
/// Die Benennung selbst steht in [`SshError::named_for_hop`] (`core`), damit
/// der Verbindungstest für Hops ab dem zweiten **dieselbe** Funktion
/// aufruft statt eines Nachbaus (Spec 0098 §7 Ebene V1; spec-reviewer Runde 1). Hier bleibt nur
/// die Übersetzung von [`Hop`] auf deren Argumente.
fn name_hop(error: SshError, hop: &Hop) -> SshError {
    error.named_for_hop(&hop.username, &hop.host, hop.port)
}

/// Was die Prüfung einer Schlüsseldatei auf Gültigkeit ergibt (Spec 0076,
/// §4.2). Bewusst **ohne** den Fehlertext der Parse-Bibliothek: Ob
/// `ssh_key`s `Display` je Eingabebytes wiedergibt, wissen wir nicht, und
/// eine Sicherheits-Invariante darf nicht am Anzeigeverhalten einer fremden
/// Kiste ruhen (5.2). Wer diesen Typ bekommt, formuliert seine eigene
/// Meldung.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyClassification {
    /// Ein gültiger OpenSSH-Schlüssel. `encrypted` ist eine
    /// **Feststellung**, kein Urteil: ob eine fehlende Passphrase ein
    /// Fehler ist, entscheidet `resolve_auth` (Spec 0076, A-5).
    Valid {
        encrypted: bool,
    },
    Invalid,
}

/// Prüft rohe Bytes auf einen gültigen OpenSSH-Privatschlüssel und meldet,
/// ob er verschlüsselt ist (Spec 0076, §4.2).
///
/// Liegt hier und nicht in `app-shell`, obwohl die `KeyFileReader`-
/// Umsetzung dort wohnt: Das Parsen von Schlüsseln ist bereits Sache dieser
/// Crate (s. [`load_private_key`] direkt darunter, dieselbe
/// `PrivateKey::from_openssh`-Zeile), und `app-shell` hat keine eigene
/// Schlüssel-Bibliothek — sie dafür aufzunehmen wäre eine neue
/// Abhängigkeit für eine Zeile, die es hier schon gibt. `core` scheidet
/// ohnehin aus (keine I/O- und keine `russh`-Abhängigkeit, Spec 0076 4.2).
pub fn classify_openssh_key(bytes: &[u8]) -> KeyClassification {
    match PrivateKey::from_openssh(bytes) {
        Ok(parsed) => KeyClassification::Valid {
            encrypted: parsed.is_encrypted(),
        },
        // Der Fehler wird bewusst fallen gelassen, nicht durchgereicht
        // (5.2) — der Aufrufer formuliert eine eigene, pfadtragende
        // Meldung ohne Dateiinhalt.
        Err(_) => KeyClassification::Invalid,
    }
}

/// Parst einen Private Key im OpenSSH-Format aus `key` (Rohtext, wie ihn
/// `CredentialStore` liefert) und entschlüsselt ihn ggf. mit `passphrase`.
/// Fehlende Passphrase bei verschlüsseltem Key ist ein Credential-
/// Auflösungsfehler, kein Panic.
fn load_private_key(
    key: &SecretString,
    passphrase: Option<&SecretString>,
) -> Result<PrivateKey, SshError> {
    let parsed = PrivateKey::from_openssh(key.expose_secret().as_bytes())
        .map_err(|e| SshError::CredentialResolutionFailed(format!("Private Key ungültig: {e}")))?;

    if !parsed.is_encrypted() {
        return Ok(parsed);
    }

    let Some(passphrase) = passphrase else {
        return Err(SshError::CredentialResolutionFailed(
            "Private Key ist verschlüsselt, aber keine Passphrase hinterlegt".to_string(),
        ));
    };

    parsed
        .decrypt(passphrase.expose_secret().as_bytes())
        .map_err(|e| {
            SshError::CredentialResolutionFailed(format!(
                "Passphrase falsch oder Key beschädigt: {e}"
            ))
        })
}

#[cfg(unix)]
async fn authenticate_via_agent(
    handle: &mut client::Handle<ClientHandler>,
    username: &str,
) -> Result<russh::client::AuthResult, SshError> {
    use russh::keys::agent::client::AgentClient;

    let mut agent = AgentClient::connect_env().await.map_err(|e| {
        SshError::CredentialResolutionFailed(format!("SSH-Agent nicht erreichbar: {e}"))
    })?;
    let identities = agent.request_identities().await.map_err(|e| {
        SshError::CredentialResolutionFailed(format!("Agent-Identitäten nicht abrufbar: {e}"))
    })?;
    if identities.is_empty() {
        return Err(SshError::CredentialResolutionFailed(
            "SSH-Agent hat keine Identitäten geladen".to_string(),
        ));
    }

    // Issue #1217: like OpenSSH, offer the identities in the order the agent
    // returns them until the server accepts one. A server disconnect (e.g.
    // `MaxAuthTries`) surfaces as an error from the call and ends the loop.
    let mut last = None;
    for identity in identities {
        let public_key = match &identity {
            russh::keys::agent::AgentIdentity::PublicKey { key, .. } => key.clone(),
            russh::keys::agent::AgentIdentity::Certificate { certificate, .. } => {
                russh::keys::PublicKey::new(certificate.public_key().clone(), "")
            }
        };
        let mut signer = AgentSigner {
            client: &mut agent,
            identity,
        };
        let result = handle
            .authenticate_publickey_with(username.to_string(), public_key, None, &mut signer)
            .await
            .map_err(crate::error::map_transport_error)?;
        match &result {
            russh::client::AuthResult::Success => return Ok(result),
            russh::client::AuthResult::Failure {
                partial_success: true,
                ..
            } => return Ok(result),
            russh::client::AuthResult::Failure { .. } => last = Some(result),
        }
    }
    // Non-empty list, so at least one failure was recorded.
    last.ok_or(SshError::AuthenticationFailed)
}

#[cfg(not(unix))]
async fn authenticate_via_agent(
    _handle: &mut client::Handle<ClientHandler>,
    _username: &str,
) -> Result<russh::client::AuthResult, SshError> {
    // `russh::keys::agent::client::AgentClient::connect_env()` ist
    // `#[cfg(unix)]` (Unix-Domain-Socket über `SSH_AUTH_SOCK`) — ein
    // Windows-Pendant existiert in `russh` zwar (`connect_pageant()`,
    // `connect_named_pipe()`), wird hier aber bewusst noch nicht angebunden.
    // Kein Panic/TODO, sondern ein ehrlicher, klar benannter Fehler. Siehe
    // ADR-Vorschlag in der Abschluss-Nachricht.
    Err(SshError::CredentialResolutionFailed(
        "SSH-Agent-Authentifizierung ist auf dieser Plattform noch nicht unterstützt".to_string(),
    ))
}

/// Dünner Wrapper, der `russh::keys::agent::client::AgentClient` als
/// `auth::Signer` nutzbar macht.
///
/// `russh` 0.63.1 implementiert `Signer` für `AgentClient` entgegen seiner
/// eigenen Doc-Kommentare ("this crate only provides an implementation for
/// an SSH agent") tatsächlich **nicht** — im Zuge dieser Implementierung im
/// Quellcode verifiziert (`grep` nach `impl.*Signer.*for AgentClient` liefert
/// keinen Treffer). Dieser Wrapper schließt die Lücke selbst: `auth_sign`
/// leitet direkt an `AgentClient::sign_request` weiter, dessen Signatur
/// bereits nahezu exakt zu `Signer::auth_sign` passt. Siehe ADR-Vorschlag in
/// der Abschluss-Nachricht.
#[cfg(unix)]
struct AgentSigner<'a> {
    client: &'a mut russh::keys::agent::client::AgentClient<tokio::net::UnixStream>,
    identity: russh::keys::agent::AgentIdentity,
}

#[cfg(unix)]
impl russh::Signer for AgentSigner<'_> {
    type Error = crate::error::TransportError;

    async fn auth_sign(
        &mut self,
        _key: &russh::keys::agent::AgentIdentity,
        hash_alg: Option<russh::keys::HashAlg>,
        to_sign: Vec<u8>,
    ) -> Result<Vec<u8>, Self::Error> {
        self.client
            .sign_request(&self.identity, hash_alg, to_sign)
            .await
            .map_err(crate::error::TransportError::Keys)
    }
}

#[cfg(test)]
mod interactive_tests {
    use super::*;
    use async_trait::async_trait;
    use ssh_manager_core::ssh::AuthPrompter;
    use std::time::Duration;

    /// Never answers.
    struct Silent;

    #[async_trait]
    impl AuthPrompter for Silent {
        async fn prompt(
            &self,
            _request: AuthPromptRequest,
        ) -> Result<Vec<SecretString>, PromptCancelled> {
            std::future::pending().await
        }
    }

    #[tokio::test(start_paused = true)]
    async fn test_silent_prompter_is_cancelled_after_five_minutes() {
        assert_eq!(AUTH_PROMPT_TIMEOUT, Duration::from_secs(300));
        let pause = AuthPause::new();
        let prompting = AuthPrompting {
            prompter: &Silent,
            pause: &pause,
        };
        let started = tokio::time::Instant::now();
        let result = ask(&prompting, AuthPromptRequest::password("u@h:22")).await;
        assert_eq!(result.unwrap_err(), PromptCancelled);
        assert_eq!(started.elapsed(), AUTH_PROMPT_TIMEOUT);
    }

    #[test]
    fn test_failure_error_names_methods_only_when_the_tried_one_is_gone() {
        let methods = |m: &[&str]| m.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            failure_error(&methods(&["keyboard-interactive"]), Some("publickey")),
            SshError::AuthenticationMethodRequired {
                methods: methods(&["keyboard-interactive"])
            }
        );
        assert_eq!(
            failure_error(&methods(&["password", "publickey"]), Some("password")),
            SshError::AuthenticationFailed
        );
        assert_eq!(
            failure_error(&[], Some("password")),
            SshError::AuthenticationFailed
        );
    }
}
