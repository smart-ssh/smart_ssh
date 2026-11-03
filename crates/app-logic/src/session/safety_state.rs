//! Safety-model state of a session: filter engine, sudo password, second
//! opinion, injection-check flags, MCP confirmation claim.

use std::sync::{Arc, Mutex as StdMutex};

use ssh_manager_core::ai::AiProvider;

use super::{CommandEvaluator, SwappableSudoPassword};

/// Issue #871: everything of a session that belongs to the safety model.
/// Moved out of `SessionParts` without changing when a field is read or
/// written; each field keeps its own lock (no lock around the whole struct).
pub struct SafetyState {
    pub filter_engine: Box<dyn CommandEvaluator>,
    /// Spec 0018, Abschnitt 6: optionales Sudo-Passwort, einmalig bei
    /// `connect()` aus dem `CredentialStore` gelesen — `None`, wenn für den
    /// Server keines hinterlegt ist (kein Fehler, s. dortiger Kommentar).
    /// Wird ausschließlich über Stdin an genau einen `sudo -S`-Aufruf
    /// weitergereicht (`crate::orchestration::execute_suggested_command`),
    /// nie auf dem Zielserver abgelegt.
    pub sudo_password: SwappableSudoPassword,
    /// Spec 0026, Abschnitt 3: `Some`, wenn die optionale KI-Zweitmeinung
    /// zur Daten-Risiko-Achse aktiviert ist UND ein gültiger, separater
    /// Provider dafür konfiguriert ist — einmalig bei `connect()` aus den
    /// `tauri-plugin-store`-Einstellungen aufgelöst (analog zu
    /// `ai_provider_label`/`ai_model` oben), nicht bei jedem Aktionsvorschlag
    /// neu gelesen. Ein während einer laufenden Session geänderter Wert
    /// greift dadurch erst bei der nächsten `connect()` — ein bewusster,
    /// kleiner Scope-Kompromiss: eine Live-Aktualisierung mitten in einer
    /// Session hätte deutlich mehr Zustands-Plumbing gebraucht, für eine
    /// reine Komfort-Einstellung unverhältnismäßig.
    pub risk_second_opinion_provider: Option<Box<dyn AiProvider>>,
    /// Spec 0061: derselbe Rate-Limit-Budget-Wächter, den der zugehörige
    /// `risk_second_opinion_provider` beim Bau (`ai_provider_factory::
    /// build_ai_provider`) aus `AppState.rate_limit_registry` bekommen hat
    /// — als eigenständiges Feld gehalten, weil `Box<dyn AiProvider>`
    /// (s. oben) keinen Weg bietet, nachträglich an seinen internen
    /// Wächter heranzukommen (die `AiProvider`-Trait-Grenze kennt Spec
    /// 0061 nicht, s. `crate::orchestration::wait_for_rate_limit_budget`,
    /// das dieses Feld VOR dem jeweiligen `send()`-Aufruf konsultiert).
    /// `Some` genau dann, wenn `risk_second_opinion_provider` `Some` ist.
    pub risk_second_opinion_budget: Option<Arc<ai_providers::ProviderBudgetGuard>>,
    /// Spec 0092, A1.3: ob ein rot eingestufter Vorschlag immer eine
    /// Bestätigung verlangt — app-weite Einstellung (`redRiskAlwaysConfirm`
    /// in `settings.json`), einmalig bei `connect()` aufgelöst, genau wie
    /// `risk_second_opinion_provider` oben. Eine Änderung greift deshalb
    /// erst bei der nächsten Verbindung; der Hinweistext am Schalter sagt
    /// das.
    ///
    /// **Fail-safe `true`** (A1.2): fehlender Schlüssel, kein boolescher
    /// Wert oder ein nicht öffenbarer Store gelten als „an". In
    /// Test-Fixtures ebenfalls `true` (Spec 0092, A1.2) — damit ein
    /// bestehender Test mit rotem Kommando und Allow-Regel sichtbar bricht,
    /// statt still auf „aus" zu laufen.
    pub red_risk_always_confirm: bool,
    /// Issue #464 (Spec 0038): the edition's transform for approved text right
    /// before it is sent (`AppState.outgoing_transform`). `None`: text is sent
    /// unchanged.
    pub outgoing_transform: Option<Arc<dyn ssh_manager_core::outgoing::OutgoingTransform>>,
    /// Spec 0039, Abschnitt 5: `true`, sobald in dieser Sitzung
    /// **irgendein** durch `ai::fence_untrusted` gelaufener Inhalt in den
    /// KI-Kontext gelangt ist (Kommando-Ausgabe, SFTP-Dateiinhalt oder
    /// Server-/Gruppen-Notiz im System-Prompt) — anders als
    /// `auto_continue_stop` **niemals** zurückgesetzt, auch nicht über neue
    /// Nutzer-Nachrichten hinweg (ersetzt damit die alte, pro-Turn
    /// zurückgesetzte SEC-03-Bremse, s. `orchestration::handle_action_
    /// proposed`-Kommentar an der Eskalationsstelle für die Begründung).
    /// Initialisiert mit `true`, wenn die Sitzung bereits mit Notizen oder
    /// (künftig, Spec 0034) vorbelasteter Historie startet — s.
    /// `history_contains_untrusted_content`.
    pub untrusted_content_ingested: std::sync::atomic::AtomicBool,
    /// Spec 0039, Abschnitt 5.1: einmalig bei `connect()` vom Server-Profil
    /// übernommen (analog zu `ai_provider_label`/`risk_second_opinion_
    /// provider` oben) — steuert, wie stark eskaliert wird, NACHDEM
    /// `untrusted_content_ingested` gesetzt ist. Das Fencing selbst
    /// (Abschnitt 3/4) läuft davon unabhängig immer.
    pub post_ingest_policy: ssh_manager_core::profiles::PostIngestPolicy,
    /// Spec 0039, Abschnitt 5.2: `Some`, wenn sowohl die app-weite
    /// Zweitmeinungs-Einstellung (Spec 0026, Abschnitt 3) als auch die
    /// serverspezifische `ai_injection_check_enabled`-Einstellung aktiv
    /// sind — einmalig bei `connect()` aufgelöst (analog zu
    /// `risk_second_opinion_provider`, derselbe konfigurierte Provider,
    /// separat aufgelöst, weil `Box<dyn AiProvider>` nicht `Clone` ist).
    pub injection_check_provider: Option<Box<dyn AiProvider>>,
    /// Spec 0061: Gegenstück zu `risk_second_opinion_budget`, für
    /// `injection_check_provider`. In der Praxis oft **derselbe** `Arc`
    /// wie `risk_second_opinion_budget` (beide werden normalerweise aus
    /// derselben `riskClassifierProviderId`-Einstellung gebaut, s.
    /// `risk_second_opinion::resolve_second_opinion_provider` — die
    /// `RateLimitRegistry` gibt für dieselbe Provider-Identität denselben
    /// Wächter zurück, unabhängig vom Aufrufzweck, s. Spec 0061 Abschnitt
    /// 2, "geteiltes Budget pro Provider-Identität").
    pub injection_check_budget: Option<Arc<ai_providers::ProviderBudgetGuard>>,
    /// Spec 0039, Abschnitt 5.2: `true`, sobald der letzte gelaufene
    /// Einschleusungs-Check "ja" ergeben hat — anders als
    /// `untrusted_content_ingested` NICHT dauerhaft-monoton, sondern
    /// "klebrig bis verbraucht": `handle_action_proposed` setzt es beim
    /// Eskalieren einer Aktion wieder auf `false` zurück, weil sich die
    /// Eskalation laut Spec auf "die auf diesem Inhalt basierende
    /// Folgeaktion" bezieht (Singular), nicht auf den Rest der Sitzung.
    pub injection_suspected: std::sync::atomic::AtomicBool,
    /// Issue #693: the checker's (redacted, length-limited) reason for the
    /// suspicion above. Display text only — shown in the confirmation that
    /// consumes the flag, never sent to the AI. Cleared together with the
    /// flag; set alongside it.
    pub injection_suspected_reason: StdMutex<Option<String>>,
    /// Issue #102: `true`, wenn der letzte Einschleusungs-Check nicht laufen
    /// konnte (Provider-Fehler) oder kein Urteil lieferte. Wie
    /// `injection_suspected` "klebrig bis verbraucht": die nächste
    /// `AutoExec`-Aktion wird zu `Confirm` und setzt es zurück (fail closed,
    /// pro Aktion); beeinflusst `injection_suspected` nie.
    pub injection_check_unavailable: std::sync::atomic::AtomicBool,
    /// Issue #102: `true`, wenn die Zweitmeinung aktiv ist, beim `connect()`
    /// aber nicht eingerichtet werden konnte; Hinweis einmalig (`swap(false)`).
    pub second_opinion_setup_notice_pending: std::sync::atomic::AtomicBool,
    /// Issue #231: `true`, wenn die serverspezifische Injection-Prüfung an
    /// ist, beim `connect()` aber kein Anbieter zur Verfügung stand
    /// (Zweitmeinung app-weit aus oder nicht einrichtbar). Nur Hinweis, keine
    /// erzwungene Bestätigung; einmalig gesendet (`swap(false)`).
    pub injection_check_inactive_notice_pending: std::sync::atomic::AtomicBool,
    /// Issue #843: notice `REDACTION_INCOMPLETE` due (secrets not fully loaded).
    pub redaction_incomplete_notice_pending: std::sync::atomic::AtomicBool,
    /// MCP-Ausschluss aus der rollierenden Summary (Nachtrag zu Spec 0057
    /// §2.1, Entscheidung: die Summary bildet Chat-Kontinuität ab,
    /// MCP-Verkehr ist kein Chat — Audit bleibt vollständig im Ledger,
    /// Etappe 1, unabhängig davon). Parallel zu `context.history` geführt
    /// (Index `i` sagt, ob `context.history[i]` MCP-originiert ist,
    /// `true` ⟺ die Nachricht wurde mit `persist: false` gepusht) —
    /// `ChatMessage` selbst trägt keine Herkunftsinformation (kein
    /// `core`-Typ soll App-Shell-Konzepte wie MCP kennen), deshalb dieser
    /// Session-lokale Spiegel. Ausschließlich von `orchestration::
    /// push_history_scoped` gepflegt, IM SELBEN `context`-Lock wie der
    /// `history.push` selbst (atomar, keine zwei Nachrichten können sich
    /// zwischen den beiden Pushes einschieben) — dieselbe Quelle der
    /// Wahrheit wie der bereits bestehende `persist`-Parameter, der auch
    /// den `chat_messages`-Persistenz-Ausschluss steuert (Spec 0034 §10),
    /// hier nur zusätzlich für die Kompaktierung gespiegelt statt eines
    /// zweiten, divergierenden Mechanismus. `StdMutex` statt `AsyncMutex`:
    /// nur kurze, nie über einen `.await`-Punkt gehaltene Push-Operationen,
    /// wie bei `pending_action`.
    pub mcp_origin_flags: StdMutex<Vec<bool>>,
    /// Issue #108, Spec 0104 §3: `true`, solange eine MCP-Bestätigung dieser
    /// Session registriert ist (von der Registrierung bis zum Aufräumen,
    /// also auch vor dem Setzen des Indikators). Ein weiterer MCP-Vorschlag
    /// wird dann abgewiesen.
    pub mcp_confirmation_claim: std::sync::atomic::AtomicBool,
}
