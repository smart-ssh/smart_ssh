//! Kernschleife eines Chat-Turns (Spec 0007, Abschnitt 6) — s. Moduldoc in
//! `orchestration.rs` für den vollständigen Kontext (Spec 0083: reine
//! Verschiebung aus `orchestration.rs`, keine Verhaltensänderung).

use futures::StreamExt;

use ssh_manager_core::ai::{AiError, AiEvent, ChatMessage, MessageContent, OutputRedactor, Role};
use ssh_manager_core::audit::{LedgerEntryContent, LedgerSource};
use ssh_manager_core::profiles::{AiAction, ProfileStore};

use crate::confirmation::ConfirmationRegistry;
use crate::dto::{ActionOrigin, ActionUserDecision};
use crate::events::{
    emit_chat_auto_continuation_limit_reached, emit_chat_auto_continuation_started,
    emit_chat_error, emit_chat_queued_messages_sent, emit_chat_response_cancelled,
    emit_chat_response_empty, emit_chat_response_truncated, emit_chat_text_delta, EventEmitter,
};
use crate::session::Session;
use crate::state::{ActionId, SessionId};

use super::action_exec::handle_action_proposed;
use super::notes::handle_document_generated;

// Testcode-Ausnahme zum `deny` — s. `orchestration.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_compaction;
// Testcode-Ausnahme zum `deny` — s. `orchestration.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_continuation;
// Testcode-Ausnahme zum `deny` — s. `orchestration.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_rounds;

/// Sicherheitsgrenze gegen eine KI, die in jeder Folgerunde erneut eine
/// Aktion vorschlägt, die wieder ausgeführt/abgelehnt/blockiert wird — ohne
/// diese Grenze könnte `run_chat_turn` sonst unbegrenzt weiterlaufen (Spec
/// 0021, Abschnitt 4: "Default-Limit 10"). Pro ursprünglicher
/// Nutzer-Nachricht: da diese Konstante nur die lokale `for`-Schleife in
/// [`run_chat_turn`] begrenzt und jeder Aufruf von `send_chat_message` (=
/// jede neue Nutzer-Nachricht) `run_chat_turn` frisch aufruft, ist kein
/// zusätzlicher, persistenter Zähler nötig — "Zähler wird bei neuer
/// Nachricht zurückgesetzt" (Spec 0021, Abschnitt 4) ergibt sich bereits
/// aus dieser Struktur von selbst.
///
/// War vor Spec 0021 auf 25 gesetzt (ursprünglich 8, dann erhöht — s.
/// `docs/adr/0014-automatic-followup-round-after-executed-action.md`), weil
/// mehrstufige, aber legitime Admin-Aufgaben mit vielen tatsächlich
/// ausgeführten Schritten sonst zu früh abbrachen. Spec 0021 zählt jetzt
/// aber **jeden** der vier Ausgänge als Runde, nicht nur ausgeführte
/// Aktionen (s. Moduldoc) — eine abgelehnte/blockierte Runde beendet sich
/// selbst quasi sofort (kein Warten auf einen entfernten Prozess), sodass
/// dieselbe 25er-Großzügigkeit hier nicht mehr nötig ist, um legitime
/// mehrstufige Aufgaben nicht vorzeitig abzuwürgen. Zusätzlich lässt sich
/// eine Kette jetzt jederzeit manuell stoppen (Abschnitt 5) statt nur auf
/// diesen Zähler angewiesen zu sein — das Erreichen des Caps ist ohnehin
/// kein Fehler, nur ein weicher Stopp mit Fortsetzungsmöglichkeit per neuer
/// Nachricht.
const MAX_AUTO_FOLLOWUP_ROUNDS: usize = 10;

/// Spec 0065, Teil 1: `max_tokens_hint` für jeden KI-Nebenaufruf, der
/// `session.ai_provider` wiederverwendet (Zweitmeinung, Injection-Check,
/// Auto-Titel, Notiz-Vorschlag/-Kürzung, Verlaufs-Zusammenfassung) — ohne
/// diesen expliziten, kleinen Wert würden diese Aufrufe den NEUEN,
/// modellabhängigen Haupt-Chat-Default erben (bis zu 128k bei aktuellen
/// Claude-Modellen), obwohl dort Kürze gewollt ist (ein Modell, das statt
/// "red"/eines kurzen Titels/einer gekürzten Notiz einen Aufsatz schreibt).
/// Bewusst identisch zum bisherigen, unveränderten `DEFAULT_MAX_TOKENS` aus
/// `ai_providers::anthropic` — der Status quo für alle fünf Nebenaufrufe
/// bleibt exakt so, wie er heute schon ist, nur jetzt EXPLIZIT statt als
/// zufälliger Nebeneffekt des (bisher einzigen) Providerweiten Defaults.
pub(crate) const SIDE_CALL_MAX_TOKENS: u32 = 4096;

/// Spec 0046, Fund 4: Backend-seitige Grundsicherung gegen eine wartende
/// `Confirm`-Aktion, die niemals aufgelöst wird — "Tab schließen = wartende
/// Aktion ablehnen" (Spec 0017, Abschnitt 5) hängt am Frontend-State, den
/// ein Reload (Dev-Hot-Reload, Frontend-Neustart) verliert; ohne dieses
/// Timeout würde der `oneshot`-Kanal aus `ConfirmationRegistry` dann nie
/// aufgelöst und dieser Task ewig auf `rx.await` warten. Bewusst großzügig
/// (nicht wie das kurze MCP-Timeout aus Spec 0028, das einen *externen
/// Tool-Aufrufer* betrifft, der typischerweise Sekunden, nicht Stunden
/// wartet) — ein Mensch soll einen riskanten Vorschlag in Ruhe prüfen
/// können, dieses Timeout ist ein Sicherheitsnetz gegen "nie", nicht eine
/// UX-Grenze gegen "lange". Läuft es ab, gilt die Aktion als **abgelehnt**
/// (fail-safe, s. `handle_action_proposed`/`handle_note_update_suggested`),
/// nie als genehmigt.
pub const PENDING_ACTION_CONFIRM_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(3600);

/// Spec 0034, Abschnitt 4: "Jede Nachricht ... wird fortlaufend
/// geschrieben, sobald sie entsteht — kein Sammeln bis zum
/// Verbindungsende." Zentrale Stelle statt an jedem der zahlreichen
/// `history.push(...)`-Aufrufer einzeln zu duplizieren — sonst reicht ein
/// vergessener Aufrufer, um eine Nachrichtenart lautlos nie zu
/// persistieren. Persistiert nur, wenn diese Session überhaupt an eine
/// `chat_sessions`-Zeile gebunden ist (`chat_session_store`/
/// `chat_session_id` beide `Some`, s. `Session`-Doc-Kommentar) — Tests
/// bleiben dadurch unverändert reines In-Memory-Verhalten. MCP-Herkunft
/// wird NICHT hier abgefangen (eine MCP-Aktion kann durchaus dieselbe,
/// bereits persistierte Session eines offenen Menschen-Tabs mitnutzen, s.
/// `mcp_backend::AppMcpBackend::ensure_session`) — dafür gibt es
/// [`push_history_scoped`] mit explizitem `persist: false`.
///
/// Ein Persistenzfehler beendet den Chat-Turn nicht (nur geloggt) — ein
/// DB-Schreibfehler mitten in einer laufenden KI-Antwort dem Nutzer als
/// harten Turn-Abbruch zu zeigen wäre unverhältnismäßig; die Nachricht
/// bleibt im In-Memory-`SessionContext` in jedem Fall korrekt sichtbar,
/// nur ihre Persistenz fehlt dann für diesen einen Eintrag.
pub async fn push_history(session: &Session, message: ChatMessage) {
    push_history_scoped(session, message, true).await;
}

/// Wie [`push_history`], mit expliziter Kontrolle darüber, ob die
/// Nachricht auch persistiert wird (Spec 0040, Abschnitt 4): `persist:
/// false` für MCP-Herkunft — Spec 0034, Abschnitt 10 verlangt, dass
/// MCP-Aktionen keine persistierte, wiederaufnehmbare Historie erzeugen,
/// selbst wenn sie (weil bereits ein Tab für denselben Server offen ist,
/// s. `mcp_backend::AppMcpBackend::ensure_session`) dieselbe `Session` samt
/// `chat_session_id` eines Menschen-Tabs mitnutzen. Der In-Memory-
/// `SessionContext` wird in JEDEM Fall aktualisiert — nur der DB-
/// Schreibzugriff wird unterdrückt, s. Aufrufer in `handle_action_
/// proposed`/`handle_user_decision`/`execute_*`.
pub(crate) async fn push_history_scoped(session: &Session, message: ChatMessage, persist: bool) {
    {
        // MCP-Ausschluss aus der rollierenden Summary (Nachtrag zu Spec
        // 0057 §2.1): `mcp_origin_flags` wird IM SELBEN `context`-Lock wie
        // der `history.push` selbst gepflegt (atomar — kein anderer
        // `push_history_scoped`-Aufruf kann sich zwischen beide Pushes
        // schieben, s. `Session::mcp_origin_flags`-Doc-Kommentar).
        // `!persist` ist exakt dieselbe Bedingung, die auch den
        // `chat_messages`-Persistenz-Ausschluss steuert (Spec 0034 §10) —
        // dieselbe Quelle der Wahrheit, kein zweiter Mechanismus.
        let mut ctx = session.context.lock().await;
        ctx.history.push(message.clone());
        // Spec 0088, A4.1: vergiftungstolerant — ein Panic hier liesse
        // `history` und `mcp_origin_flags` dauerhaft verschieden lang
        // zurück, und die Kompaktierung müsste jede Nachricht defensiv als
        // MCP-originiert behandeln (s. `group_mcp_flags_by_round`).
        crate::poison::lock_tolerating_poison(&session.mcp_origin_flags).push(!persist);
    }
    if !persist {
        return;
    }

    let Some(store) = &session.chat_session_store else {
        return;
    };
    let Some(chat_session_id) = *session.chat_session_id.lock().await else {
        return;
    };
    if let Err(err) = store.append_message(chat_session_id, &message).await {
        tracing::warn!(error = %err, "chat message persistence failed");
    }
}

/// Ableitung der [`LedgerSource`] eines Ledger-Eintrags aus der
/// [`ActionOrigin`] eines Aktionsvorschlags (Spec 0057, §1.1). Es gibt
/// bewusst keinen dritten `ActionOrigin`-Wert für "manuell" — ein
/// `AiAction`-Vorschlag kommt immer entweder aus dem internen Chat
/// (`Internal`, von der KI) oder von einem MCP-Client, nie direkt von
/// einem Menschen (der bestätigt/lehnt nur ab, s. `LedgerSource::User`,
/// das ausschließlich in `handle_user_decision` vergeben wird, nicht
/// hier).
pub(crate) fn ledger_source_for_origin(origin: &ActionOrigin) -> LedgerSource {
    match origin {
        ActionOrigin::Internal => LedgerSource::Ai,
        ActionOrigin::Mcp { .. } => LedgerSource::McpAgent,
    }
}

/// Redigiert alle Inhalts-tragenden Varianten eines [`LedgerEntryContent`]
/// (Spec 0057, §1.2 "PFLICHT") — zentral hier statt an jeder Aufrufstelle
/// von [`write_ledger_entry`] einzeln, damit ein vergessener Aufrufer
/// keinen unredigierten Eintrag durchlassen kann. `Decision` trägt keine
/// Ausgabedaten (nur `reason`/`code`, von der Filter-Engine selbst
/// erzeugte Texte, kein Serverinhalt) und bleibt deshalb unverändert.
fn redact_ledger_entry_content(
    redactor: &dyn OutputRedactor,
    content: LedgerEntryContent,
) -> LedgerEntryContent {
    match content {
        LedgerEntryContent::CommandProposed { command } => LedgerEntryContent::CommandProposed {
            command: redactor.redact_text(&command),
        },
        LedgerEntryContent::CommandExecuted {
            command,
            output,
            cancelled,
        } => LedgerEntryContent::CommandExecuted {
            command: redactor.redact_text(&command),
            output: redactor.redact(&output),
            cancelled,
        },
        LedgerEntryContent::AiMessage { text } => LedgerEntryContent::AiMessage {
            text: redactor.redact_text(&text),
        },
        decision @ LedgerEntryContent::Decision { .. } => decision,
    }
}

/// Zentrale Schreibstelle für das Session-Ledger (Spec 0057, §1) — analog
/// zu [`push_history`]/[`push_history_scoped`] oben, aber bewusst NICHT
/// über deren `persist`-Flag/-Gate laufend: jenes Flag schließt MCP-
/// Herkunft absichtlich aus der wiederaufnehmbaren Chat-Historie aus (Spec
/// 0034, Abschnitt 10), das Ledger soll MCP-Aktivität aber gerade erfassen
/// (Spec 0057, §1.1: Quelle `mcp-agent`) — deshalb ein eigenes, von
/// `persist` unabhängiges Gate: nur `session.ledger_store`/
/// `session.chat_session_id` müssen gesetzt sein.
///
/// Redigiert `content` selbst (s. [`redact_ledger_entry_content`]) —
/// Aufrufer übergeben unredigierte Daten. Ein Persistenzfehler bricht den
/// Chat-Turn nicht ab (nur geloggt), aus demselben Grund wie bei
/// `push_history_scoped`.
pub(crate) async fn write_ledger_entry(
    session: &Session,
    source: LedgerSource,
    content: LedgerEntryContent,
) {
    let Some(store) = &session.ledger_store else {
        return;
    };
    let Some(chat_session_id) = *session.chat_session_id.lock().await else {
        return;
    };
    let redacted = redact_ledger_entry_content(session.redactor.as_ref(), content);
    if let Err(err) = store.append_entry(chat_session_id, source, &redacted).await {
        tracing::warn!(error = %err, "ledger entry persistence failed");
    }
}

/// Spec 0051, Teil 2: Mindestabstand zwischen zwei `AiProvider::send()`-
/// Aufrufen derselben Sitzung. Alle Aufrufstellen (Haupt-Chat, Risiko-
/// Zweitmeinung, Einschleusungs-Check, Auto-Titel, Notiz-Vorschlag) sind
/// bereits strikt seriell — jede einzelne wird vollständig `.await`et,
/// bevor die nächste beginnt (s. Spec 0051, Teil-0-Diagnosebericht) —, aber
/// ohne jeden zeitlichen Abstand: eine Anfrage, die unmittelbar nach einem
/// 429 der vorherigen abgeschickt wird, trifft in der Praxis dasselbe
/// Rate-Limit-Fenster (im real reproduzierten Fall lagen nur 31ms
/// dazwischen). 300ms sind bewusst klein genug, um für den Nutzer nicht
/// spürbar zu sein (deutlich unter der Reaktionszeit, die ein Streaming-
/// Antwortbeginn ohnehin braucht), aber groß genug, um zwei App-intern
/// ausgelöste Anfragen aus demselben Burst zeitlich zu trennen.
const MIN_AI_REQUEST_SPACING: std::time::Duration = std::time::Duration::from_millis(300);

/// Wartet bei Bedarf, bis [`MIN_AI_REQUEST_SPACING`] seit dem letzten
/// `AiProvider::send()`-Aufruf dieser Sitzung vergangen ist, und
/// aktualisiert danach den Zeitstempel — s. `Session::ai_request_paced_at`-
/// Doc-Kommentar zur Begründung, warum dies unabhängig von der konkreten
/// `AiProvider`-Instanz gilt (Haupt-Provider vs. separat konfigurierter
/// Zweitmeinungs-/Einschleusungs-Check-Provider können dasselbe Rate-
/// Limit-Kontingent teilen). Aufzurufen unmittelbar vor jedem `send()`
/// dieser Sitzung, nicht danach — sonst würde die Wartezeit die ohnehin
/// schon laufende Anfrage nicht mehr entzerren.
pub(crate) async fn wait_for_ai_request_slot(session: &Session) {
    let mut paced_at = session.ai_request_paced_at.lock().await;
    if let Some(previous) = *paced_at {
        let elapsed = previous.elapsed();
        if elapsed < MIN_AI_REQUEST_SPACING {
            tokio::time::sleep(MIN_AI_REQUEST_SPACING - elapsed).await;
        }
    }
    *paced_at = Some(tokio::time::Instant::now());
}

/// Spec 0061, Abschnitt 3: proaktives Warten VOR einem `AiProvider::
/// send()`-Aufruf, wenn der Rate-Limit-Budget-Wächter dieser
/// Provider-Identität (aus den zuletzt gelesenen `anthropic-ratelimit-*`-
/// Headern, s. `ai_providers::rate_limit_budget`) das für nötig hält —
/// **ergänzt** `wait_for_ai_request_slot` (reines Mindest-Pacing) und das
/// bestehende reaktive Retry (Spec 0051), ersetzt keins von beidem: ein
/// header-loser Provider (s. dortige Invariante) liefert hier immer
/// `None` und wird nie blockiert; ein zu knapp geschätztes/verpasstes
/// proaktives Warten fängt das reaktive 429-Retry weiterhin ab. Aufrufer
/// übergibt den zur jeweiligen `AiProvider`-Instanz gehörenden Wächter
/// (`session.ai_provider_budget`/`risk_second_opinion_budget`/
/// `injection_check_budget` — s. `Session`-Doc-Kommentare) und eine
/// Schätzung der Input-Tokens dieses konkreten Requests (`crate::
/// compaction::estimate_request_tokens`, „denselben Schätzer
/// wiederverwenden" laut Spec 0061 Abschnitt 3).
pub(crate) async fn wait_for_rate_limit_budget(
    budget: &ai_providers::ProviderBudgetGuard,
    estimated_input_tokens: usize,
    emitter: &dyn EventEmitter,
    session_id: SessionId,
) {
    if let Some(wait) = budget.wait_duration(estimated_input_tokens as u64) {
        // `as_secs_f64().ceil()`, nicht `as_secs()`: Letzteres würde eine
        // Wartezeit unter 1s zu "in 0s…" abrunden (spec-reviewer Fund) —
        // die UI soll nie "sofort" suggerieren, während die App tatsächlich
        // noch wartet.
        crate::events::emit_ai_budget_waiting(
            emitter,
            session_id,
            wait.as_secs_f64().ceil() as u64,
        );
        tokio::time::sleep(wait).await;
    }
}

/// Die Nutzer-Nachricht muss bereits vom Aufrufer in
/// `session.context.history` eingetragen worden sein (s.
/// `app_shell::commands::send_chat_message`). Läuft so lange in Folgerunden
/// weiter, wie die jeweils letzte Runde zu einem der vier Ausgänge aus Spec
/// 0021, Abschnitt 3 geführt hat (s. Moduldoc), höchstens aber
/// [`MAX_AUTO_FOLLOWUP_ROUNDS`] Runden, und bricht sofort ab, sobald
/// `Session::auto_continue_stop` gesetzt ist (Spec 0021, Abschnitt 5).
///
/// `#[tracing::instrument]` (Spec 0016, Abschnitt 2/4): trägt `session_id`
/// als Span-Feld auf jede innerhalb dieses Aufrufs geloggte Zeile ein —
/// auch auf die von `ai-providers` beim Pollen des zurückgegebenen Streams
/// (derselbe Thread-lokale Span-Stack gilt über Crate-Grenzen hinweg), ohne
/// dass `ai-providers` selbst je `session_id` kennen müsste. `skip_all`:
/// `session`/`emitter`/`profile_store`/`action_confirmations` implementieren
/// kein sinnvolles `Debug` für ein Log-Feld.
#[tracing::instrument(skip_all, fields(session_id = %session_id))]
pub async fn run_chat_turn(
    session: &Session,
    session_id: SessionId,
    emitter: &dyn EventEmitter,
    profile_store: &dyn ProfileStore,
    action_confirmations: &ConfirmationRegistry<ActionId, ActionUserDecision>,
) -> bool {
    // Das Stopp-Flag wird NICHT hier zurückgesetzt, sondern in
    // `app_shell::commands::send_chat_message_impl` atomar mit dem Start des
    // Turns (Spec 0066, spec-reviewer-Fund: ein Reset erst hier, nach
    // mehreren DB-Zugriffen, verschluckte einen früh gedrückten Stopp).
    for round in 1..=MAX_AUTO_FOLLOWUP_ROUNDS {
        let mut injected_queued_messages = false;
        if round > 1 {
            // Stopp zwischen zwei Runden: keine weitere Runde. Eingereihte
            // Nachrichten bleiben dann in der Warteschlange und werden vom
            // Aufrufer als neuer Turn gesendet (Spec 0066, §1).
            if session
                .auto_continue_stop
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                return false;
            }
            // Spec 0066, §2: während der vorigen Runde eingereihte
            // Nutzer-Nachrichten gehen mit dieser Anfrage mit — als ganz
            // normale Nutzer-Nachrichten im Verlauf (außerhalb jeder
            // Fence, selber Pfad wie `send_chat_message_impl`). Nur hier
            // UND in `send_chat_message_impl` entnommen, nie aus Tool-Output.
            let queued = session.take_queued_user_messages();
            if !queued.is_empty() {
                injected_queued_messages = true;
                for text in queued {
                    push_history(
                        session,
                        ChatMessage {
                            role: Role::User,
                            content: MessageContent::Text(text),
                        },
                    )
                    .await;
                }
                emit_chat_queued_messages_sent(emitter, session_id);
            }
            emit_chat_auto_continuation_started(emitter, session_id, round);
        }

        // Spec 0080, A2, Klarstellung Q-BL-0259-01 (2026-09-24,
        // Variante b): "leere Runde" gilt nur für eine Runde, die eine
        // Nutzer-Nachricht beantwortet — Runde 1 dieses Turns (neue
        // Nachricht oder „Weiter") sowie jede spätere Runde, in die
        // gerade eingereihte Nutzer-Nachrichten eingespeist wurden. EINE
        // automatische Folgerunde nach einer ausgeführten oder geblockten
        // Aktion (Spec 0021, Abschnitt 3, Fall 4), die ohne Text endet,
        // bleibt bewusst still — das Aktions-Ergebnis-Kärtchen ist dort
        // bereits die Antwort, und A1s eigener Retry (im Provider, je
        // Anfrage) deckt eine wegen des Längenlimits leere Folgerunde
        // bereits unabhängig davon ab.
        let check_for_empty_response = round == 1 || injected_queued_messages;

        match run_one_round(
            session,
            session_id,
            emitter,
            profile_store,
            action_confirmations,
            check_for_empty_response,
        )
        .await
        {
            RoundOutcome::Continue => {}
            RoundOutcome::Finished => return false,
            // spec-reviewer-Fund (Spec 0066): wurde in dieser Runde eine
            // eingereihte Nachricht in den Verlauf gelegt, der Request aber
            // vor dem Versand gestoppt, hat die KI sie nie gesehen — der
            // Aufrufer muss sie mit einem Folge-Turn beantworten lassen.
            RoundOutcome::StoppedBeforeSend => return injected_queued_messages,
        }
    }

    emit_chat_auto_continuation_limit_reached(emitter, session_id, MAX_AUTO_FOLLOWUP_ROUNDS);
    false
}

/// Ausgang von [`run_one_round`].
#[derive(Debug, PartialEq, Eq)]
enum RoundOutcome {
    /// Mindestens eine Aktion wurde ausgeführt — eine Folgerunde folgt.
    Continue,
    /// Runde regulär beendet (oder während des Streams gestoppt).
    Finished,
    /// Stopp, bevor der Request überhaupt rausging (Spec 0066, §1).
    StoppedBeforeSend,
}

/// Spec 0040, Abschnitt 5: wird auf die (bereits kompaktierte, s.
/// `compaction::compact_for_send`) Kopie der Historie angewendet, die
/// tatsächlich an [`ssh_manager_core::ai::AiProvider::send`] geht —
/// unmittelbar vor jedem `send()`-Aufruf, an allen drei Aufrufstellen in
/// diesem Modul.
///
/// **Kritisch — nur additiv:** wendet denselben [`OutputRedactor`] erneut
/// auf jede Nachricht an, der auch beim ursprünglichen Erzeugen bereits
/// lief (Spec 0006, Abschnitt 5, für `CommandResult`; hier neu auch für
/// freien `Text`-Inhalt über [`OutputRedactor::redact_text`]). Das ist
/// strukturell garantiert additiv: `redact`/`redact_text` ersetzen
/// ausschließlich neu erkannte Muster durch den `[REDACTED]`-Platzhalter,
/// nie umgekehrt — ein bereits vorhandener Platzhalter matcht selbst keines
/// der Muster erneut, bleibt also unverändert stehen, und kein Aufruf hier
/// kann jemals Text sichtbar machen, der vorher (in `session.context`/der
/// DB) redigiert war. Nur diese eine Kopie wird verändert — `session.
/// context`/die persistierte Historie bleiben unangetastet, exakt wie bei
/// der Kürzung nebenan.
///
/// `command`-Felder (bei `CommandResult`/`ActionRejected`) bleiben bewusst
/// unangetastet — Spec 0018, Abschnitt 5: das ausgeführte Kommando selbst
/// ist "bewusst voll transparent", nur seine Ausgabe läuft durch den
/// Redactor (s. `execute_suggested_command`).
///
/// **Fencing-Sicherheit (Nachtrag, unabhängiger Review-Pass zu Spec
/// 0040):** `MessageContent::Text` kann bereits gefencten Inhalt tragen
/// (z. B. `execute_read_remote_file`s `<remote_file>...</remote_file>`,
/// Spec 0039) — anders als `CommandResult` (dessen `output` erst *nach*
/// dieser Funktion, beim eigentlichen Request-Aufbau in `ai-providers`,
/// gefenced wird, Redaction dort also bereits strukturell vor dem
/// Fencing läuft). Ein einfacher `redact_text(&text)` über die GESAMTE,
/// bereits gefencte Zeichenkette würde ein gieriges Redaction-
/// Fallback-Muster (z. B. das Private-Key-Rückfallmuster für einen
/// abgeschnittenen Key-Block ohne `END`-Marker, s.
/// `ssh_manager_core::ai::redactor`) erlauben, über die Fence-Grenze
/// hinweg bis zum schließenden Tag zu matchen und diesen mit zu
/// verschlucken — die Fencing-Garantie aus Spec 0039 (nicht
/// vertrauenswürdiger Inhalt bleibt sicher eingerahmt) wäre damit
/// verletzt, obwohl die Redaction-Richtung selbst sicher bleibt (nichts
/// wird sichtbar, nur zusätzlich gelöscht).
///
/// Ein reines Vorziehen des Fencings vor die Redaction (der eigentlich
/// bevorzugte Fix) ist für `RemoteFile`-Inhalt hier nicht sauber möglich:
/// der gefencte Text IST die persistierte Repräsentation (`session.
/// context`/die DB speichern bereits das fertig gefencte `MessageContent::
/// Text`, s. `execute_read_remote_file`) — ein wiederaufgenommener Chat
/// redigiert beim nächsten Versand also zwangsläufig bereits gefencten
/// Text erneut. Stattdessen behandelt `redact_text_preserving_fence_
/// markers` unten die bekannten, festen Fence-Marker-Strings
/// (`ssh_manager_core::ai::fence_markers`) als Segment-Grenzen: der Text
/// wird an jedem Marker aufgetrennt, jedes Segment *zwischen* zwei
/// Markern unabhängig redigiert (ein gieriges Muster kann dadurch nie
/// über einen Marker hinausmatchen — die Marker selbst durchlaufen den
/// Redactor gar nicht erst), die Marker unverändert wieder eingefügt. Das
/// ist sicher, weil `fence_untrusted`s Escaping (`escape_for_prompt_fence`)
/// garantiert, dass ein literales `<`/`>` in bereits gefenctem Text NIE
/// aus dem ursprünglichen Inhalt stammt, sondern ausschließlich von den
/// Fence-Tags selbst — für tatsächlich gefencten Inhalt kann die
/// Marker-Liste also nichts fälschlich "beschützen", das eigentlich
/// redigiert werden müsste.
///
/// **Bekannte, bewusst nicht geschlossene Restlücke** (unabhängiger
/// Review-Pass zu diesem Fix selbst, dokumentiert statt stillschweigend
/// verschwiegen): `MessageContent::Text` trägt auch gewöhnlichen, nie
/// escapten Chat-Text (Nutzer-Eingabe, KI-Antworttext über
/// `flush_text_buffer`) — enthält so ein Text zufällig eine Marker-artige
/// Teilzeichenkette (z. B. ein zitiertes `</stdout>` ohne jeden echten
/// Fence-Bezug) MITTEN in einem unterminierten Fail-safe-Treffer (etwa
/// einem von der KI ausgegebenen, abgeschnittenen Private-Key-Block ohne
/// `END`-Marker), trennt das Segmentieren unten die Fail-safe-Reichweite
/// an dieser Stelle künstlich ab — der Teil hinter dem Marker enthält
/// dann keinen `BEGIN`-Header mehr, matcht kein Muster mehr und bleibt
/// unredigiert. Das ist eine echte, aber eng begrenzte Abschwächung
/// gegenüber dem Verhalten vor diesem Fix (der bis dahin den gesamten
/// Text ungeteilt redigierte). Bewusst nicht mitgelöst: aus reinem
/// String-Inhalt lässt sich nicht zuverlässig unterscheiden, ob ein
/// Marker-Vorkommen aus einem echten Fence stammt oder zufällig in nie
/// gefenctem Text auftaucht (das wäre nur mit einer strukturellen
/// Kennzeichnung "dieser Text ist gefenct" lösbar, die über die
/// Persistenz hinweg erhalten bliebe — ein größerer Umbau, nicht Teil
/// dieses Fixes) — und die beiden Fehlerrichtungen widersprechen sich:
/// bevorzugt man "immer volle Fail-safe-Reichweite", bricht das
/// wiederum genau den hier eigentlich zu behebenden Fence-Bruch bei
/// echtem gefenctem Inhalt. Die Redaction-Richtung bleibt in diesem
/// Rand-Rand-Fall weiterhin sicher (nichts wird sichtbar gemacht, was
/// vorher nicht sichtbar war) — nur die Reichweite ist geringer als im
/// theoretischen Idealfall.
pub(crate) fn reapply_redaction_for_send(
    history: Vec<ChatMessage>,
    redactor: &dyn OutputRedactor,
) -> Vec<ChatMessage> {
    history
        .into_iter()
        .map(|message| ChatMessage {
            role: message.role,
            content: match message.content {
                MessageContent::Text(text) => {
                    MessageContent::Text(redact_text_preserving_fence_markers(&text, redactor))
                }
                MessageContent::CommandResult {
                    command,
                    output,
                    cancelled,
                } => MessageContent::CommandResult {
                    command,
                    output: redactor.redact(&output),
                    cancelled,
                },
                other @ MessageContent::ActionRejected { .. } => other,
            },
        })
        .collect()
}

/// s. `reapply_redaction_for_send`-Doc-Kommentar ("Fencing-Sicherheit").
/// Trennt `text` an jedem bekannten Fence-Marker (`ssh_manager_core::ai::
/// fence_markers`) auf, redigiert nur die Segmente dazwischen und fügt die
/// Marker unverändert wieder ein — ein Redaction-Muster kann dadurch nie
/// über eine Fence-Grenze hinausmatchen, unabhängig davon, wie gierig es
/// ist. Findet `text` keinen Marker (der Normalfall für gewöhnlichen
/// Chat-Text), verhält sich das identisch zu einem einzelnen
/// `redactor.redact_text(text)`-Aufruf.
fn redact_text_preserving_fence_markers(text: &str, redactor: &dyn OutputRedactor) -> String {
    let markers = ssh_manager_core::ai::fence_markers();
    let mut result = String::new();
    let mut remaining = text;

    loop {
        let earliest = markers
            .iter()
            .filter_map(|marker| remaining.find(marker.as_str()).map(|idx| (idx, marker)))
            .min_by_key(|(idx, _)| *idx);

        match earliest {
            Some((idx, marker)) => {
                let (before, after) = remaining.split_at(idx);
                result.push_str(&redactor.redact_text(before));
                result.push_str(marker);
                remaining = &after[marker.len()..];
            }
            None => {
                result.push_str(&redactor.redact_text(remaining));
                break;
            }
        }
    }

    result
}

/// Genau eine KI-Antwortrunde, s. [`RoundOutcome`].
///
/// `check_for_empty_response` (Spec 0080, A2, Klarstellung Q-BL-0259-01):
/// `true` nur für eine Runde, die eine Nutzer-Nachricht beantwortet — der
/// Aufrufer [`run_chat_turn`] setzt das für Runde 1 eines Turns sowie für
/// jede Runde, in die gerade eingereihte Nutzer-Nachrichten eingespeist
/// wurden, `false` für jede automatische Folgerunde nach einer
/// ausgeführten/geblockten Aktion.
async fn run_one_round(
    session: &Session,
    session_id: SessionId,
    emitter: &dyn EventEmitter,
    profile_store: &dyn ProfileStore,
    action_confirmations: &ConfirmationRegistry<ActionId, ActionUserDecision>,
    check_for_empty_response: bool,
) -> RoundOutcome {
    let mut request_context = session.context.lock().await.clone();
    // spec-reviewer-Fund (Review dieses Schritts, Etappe 3): geklont statt
    // den `MutexGuard` über den (jetzt potenziell lange laufenden,
    // Zusammenfassungs-KI-Aufruf enthaltenden) `compact_for_send`-Aufruf
    // hinweg zu halten — ein Rust-Temporary in Argumentposition lebt sonst
    // bis zum Ende der GESAMTEN Anweisung, würde `system_context_parts`
    // also für die volle Dauer des `.await` sperren und jeden
    // gleichzeitigen Lese-/Schreibzugriff (z. B. `send_chat_message_impl`
    // bei einer neuen Nutzer-Nachricht in einem anderen Tab derselben
    // Sitzung) bis zu `SUMMARY_CALL_TIMEOUT` blockieren.
    let system_context_parts = session.system_context_parts.lock().await.clone();
    // Spec 0057, §3: "vor jedem `AiProvider::send()`-Aufruf" — kompaktiert
    // nur die an den Provider gesendete Kopie, die gespeicherte Historie in
    // `session.context`/der DB bleibt unangetastet (s.
    // `compaction::compact_for_send`-Moduldoc).
    request_context = crate::compaction::compact_for_send(
        session,
        session_id,
        emitter,
        request_context,
        &system_context_parts,
        session.model_context_window_tokens,
    )
    .await;
    // Spec 0040, Abschnitt 5: nur-additive Re-Redaction unmittelbar vor dem
    // `send()`-Aufruf, s. `reapply_redaction_for_send`-Doc-Kommentar.
    request_context.history =
        reapply_redaction_for_send(request_context.history, session.redactor.as_ref());
    // Spec 0066, §1: auch die Wartezeiten vor dem Send sind per Stopp
    // abbrechbar (beide schlafen nur, Abbruch ist folgenlos).
    let estimated_tokens = crate::compaction::estimate_request_tokens(&request_context);
    let stopped_before_send = tokio::select! {
        biased;
        () = session.auto_continue_stop_requested() => true,
        () = async {
            wait_for_ai_request_slot(session).await;
            // Spec 0061, Abschnitt 3: proaktives Rate-Limit-Gate, direkt vor
            // dem Send — nach der Kompaktierung/Redaction, damit die
            // Schätzung den tatsächlich gesendeten Request widerspiegelt.
            wait_for_rate_limit_budget(
                &session.ai_provider_budget,
                estimated_tokens,
                emitter,
                session_id,
            )
            .await;
        } => false,
    };
    if stopped_before_send {
        emit_chat_response_cancelled(emitter, session_id);
        return RoundOutcome::StoppedBeforeSend;
    }
    let mut stream = session.ai_provider.send(request_context);

    let mut text_buffer = String::new();
    let mut executed_action = false;
    // Spec 0080, A2: unabhängig von `text_buffer` (das ein `ActionProposed`
    // schon vorher per `flush_text_buffer` leert) und von `executed_action`
    // (die nur eine tatsächlich AUSGEFÜHRTE Aktion zählt) — A2 verlangt
    // "Text angefallen ODER eine Aktion vorgeschlagen", unabhängig davon,
    // ob die Aktion später abgelehnt/blockiert wurde. Einmal `true`, bleibt
    // es für den Rest dieser Runde `true`. Nur relevant, wenn
    // `check_for_empty_response` überhaupt gilt (s. Funktions-Doc-Kommentar).
    let mut round_had_content = false;
    // Spec 0068, Teil 4 (Review-Fund): wurde eine Aktion DIESER Antwort
    // abgelehnt/blockiert, läuft keine weitere Aktion derselben Antwort mehr
    // ohne Rückfrage (s. `handle_action_proposed`).
    let response_had_rejection = std::sync::atomic::AtomicBool::new(false);

    // Diagnose "KI antwortet nicht" (2026-09, Nutzerbericht): grenzt ein,
    // ob dieser `run_one_round`-Task hier überhaupt zum ersten Poll des
    // Streams kommt (ein Live-Repro zeigte: `log_outgoing_context` in
    // `ai-providers` feuerte, aber laut `lsof` nie eine Verbindung zum
    // Provider — diese Zeile grenzt ein, ob der Abbruch schon VOR diesem
    // Punkt liegt, z. B. beim vorherigen `wait_for_ai_request_slot`/
    // `compact_for_send`, oder erst im Stream selbst).
    tracing::debug!(session_id = %session_id, "about to poll AI provider stream for the first time");

    loop {
        // Spec 0066, §1: Stopp gewinnt (`biased`) gegen jedes weitere
        // Stream-Event — nach einem Stopp wird kein Event mehr verarbeitet,
        // insbesondere kein `ActionProposed`. Das anschließende Drop des
        // Streams schließt die HTTP-Verbindung und verwirft auch eine
        // gerade laufende Retry-/Backoff-Wartezeit aus `ai_providers`
        // (kein weiterer Request nach Stopp).
        let next = tokio::select! {
            biased;
            () = session.auto_continue_stop_requested() => None,
            event = stream.next() => Some(event),
        };
        let Some(event) = next else {
            drop(stream);
            flush_text_buffer(session, &mut text_buffer).await;
            emit_chat_response_cancelled(emitter, session_id);
            return RoundOutcome::Finished;
        };
        let Some(event) = event else { break };
        match event {
            AiEvent::TextDelta(delta) => {
                if !delta.is_empty() {
                    round_had_content = true;
                }
                emit_chat_text_delta(emitter, session_id, delta.clone());
                text_buffer.push_str(&delta);
            }
            AiEvent::ActionProposed(AiAction::GenerateDocument {
                title,
                content_markdown,
            }) => {
                // Spec 0080, A2: "eine Aktion vorgeschlagen" gilt auch hier
                // — der Nutzer sieht sofort ein Dokument, diese Runde ist
                // alles andere als leer.
                round_had_content = true;
                // Spec 0012, Abschnitt 2/3: läuft weder durch die
                // Filter-Engine noch durch `handle_action_proposed`s
                // Confirm-Pfad — reiner lokaler Inhalt, direkt ans Frontend
                // weitergereicht.
                flush_text_buffer(session, &mut text_buffer).await;
                handle_document_generated(session, session_id, title, content_markdown, emitter)
                    .await;
            }
            AiEvent::ActionProposed(action) => {
                // Spec 0080, A2: gilt schon bei der Vorschlags-Absicht,
                // unabhängig vom späteren Ausgang (abgelehnt/blockiert
                // zählt genauso wie ausgeführt — anders als
                // `executed_action` unten, das nur die tatsächliche
                // Ausführung für die Auto-Folgerunden-Logik zählt).
                round_had_content = true;
                flush_text_buffer(session, &mut text_buffer).await;
                if handle_action_proposed(
                    session,
                    session_id,
                    action,
                    emitter,
                    profile_store,
                    action_confirmations,
                    ActionOrigin::Internal,
                    &response_had_rejection,
                )
                .await
                {
                    executed_action = true;
                }
            }
            AiEvent::Done => {
                flush_text_buffer(session, &mut text_buffer).await;
                // Spec 0080, A2, Klarstellung Q-BL-0259-01 (Variante b): nur
                // bei `Done` (nicht bei `TextTruncated`/`Error`, s.
                // jeweiliger Zweig), nur wenn diese Runde wirklich nichts
                // hervorgebracht hat, UND nur, wenn diese Runde überhaupt
                // eine Nutzer-Nachricht beantwortet (`check_for_empty_
                // response`, s. Funktions-Doc-Kommentar) — eine automatische
                // Folgerunde nach einer ausgeführten/geblockten Aktion
                // bleibt bewusst still. Nichts davon geht in
                // Ledger/Historie (das passiert bereits vorher/gar nicht,
                // dieses Event ist rein informativ fürs Frontend).
                if check_for_empty_response && !round_had_content {
                    emit_chat_response_empty(emitter, session_id);
                }
                break;
            }
            AiEvent::TextTruncated => {
                // Spec 0065, Teil 2: der bis hierhin gestreamte Text bleibt
                // gültig und sichtbar (genau wie bei `Done`) — zusätzlich
                // ein eigenes Event, das das Frontend in einen Hinweis samt
                // „Weiter"-Aktion übersetzt, statt Text in den Inhalt zu
                // mischen (s. `emit_chat_response_truncated`-Doc-Kommentar).
                flush_text_buffer(session, &mut text_buffer).await;
                emit_chat_response_truncated(emitter, session_id);
                break;
            }
            AiEvent::Error(err) => {
                flush_text_buffer(session, &mut text_buffer).await;
                emit_chat_error(
                    emitter,
                    session_id,
                    describe_ai_error(&err),
                    Some(err.code()),
                );
                break;
            }
        }
    }

    if executed_action {
        RoundOutcome::Continue
    } else {
        RoundOutcome::Finished
    }
}

fn describe_ai_error(err: &AiError) -> String {
    err.to_string()
}

async fn flush_text_buffer(session: &Session, buffer: &mut String) {
    if buffer.is_empty() {
        return;
    }
    let text = std::mem::take(buffer);
    // Spec 0057, §1.1, vierter Punkt: "KI-Nachricht". Immer
    // `LedgerSource::Ai` — dieser Text ist buchstäblich die vom
    // `AiProvider` gestreamte Antwort, unabhängig davon, ob die auslösende
    // Aktion über den internen Chat oder MCP kam (Letzteres läuft ohnehin
    // nie über `run_one_round`, s. `app_shell::mcp_backend`).
    write_ledger_entry(
        session,
        LedgerSource::Ai,
        LedgerEntryContent::AiMessage { text: text.clone() },
    )
    .await;
    push_history(
        session,
        ChatMessage {
            role: Role::Assistant,
            content: MessageContent::Text(text),
        },
    )
    .await;
}
