//! Gemeinsame Logging-Helfer für Spec 0016 ("Strukturiertes Logging &
//! Diagnose"), Abschnitt 4, Punkte 1–3 — geteilt zwischen
//! `AnthropicProvider` und `OpenAiCompatibleProvider`, damit das Format
//! nicht zwischen beiden auseinanderläuft (analog zu `crate::error`).
//!
//! `request_id` korreliert alle Log-Zeilen *eines* `AiProvider::send()`-
//! Aufrufs (s. `crate::anthropic::AnthropicProvider::send`-Kommentar zur
//! Design-Entscheidung, warum diese ID lokal pro Aufruf erzeugt wird statt
//! ein `SessionContext`-Feld zu sein).

use serde_json::Value;
use ssh_manager_core::ai::{
    default_log_redactor, AiError, MessageContent, OutputRedactor, RejectionReason, SessionContext,
};
use ssh_manager_core::profiles::AiAction;
use uuid::Uuid;

/// Spec 0094, A1.5: Höchstlänge des Provider-`body`, der als **Ausnahme**
/// weiterhin auf `warn` stehen bleibt. Spiegelt ein Provider Teile der
/// Anfrage in seiner Fehlermeldung, bleibt der Schaden auf diese Länge
/// begrenzt (Restrisiko, in der Spec benannt).
const MAX_LOGGED_BODY_LEN: usize = 512;

/// Kürzt auf `MAX_LOGGED_BODY_LEN` Zeichen.
///
/// **Reihenfolge ist sicherheitsrelevant**: Diese Funktion wird
/// ausschließlich auf schon redigierten Text angewandt. Umgekehrt — erst
/// kürzen, dann redigieren — würde die Kürzung ein Secret-Muster mitten
/// entzweischneiden, sodass kein Muster mehr greift und der Anfang des
/// Geheimnisses im Klartext stehen bliebe.
fn truncate_logged_body(redacted: &str) -> String {
    if redacted.chars().count() <= MAX_LOGGED_BODY_LEN {
        return redacted.to_string();
    }
    let head: String = redacted.chars().take(MAX_LOGGED_BODY_LEN).collect();
    format!("{head}… (gekürzt, vollständig nur auf debug)")
}

/// Spec 0094, A2: Muster-basierte Redaction für die `debug`-Zeilen, die den
/// Inhalt tragen, den A1 aus den Zeilen ab `info` entfernt. In dieser Crate
/// gibt es keinen Session-Redactor, deshalb der prozessweite
/// [`default_log_redactor`] (eine Instanz, nicht je Aufruf neu gebaut).
///
/// Zusätzlich zu, nicht anstelle von [`redact_secrets`]: Letzteres ersetzt
/// die wörtlich bekannten eigenen Geheimnisse (API-Key,
/// `extra_headers`-Werte), der Redactor die allgemeinen Muster. Beide
/// Richtungen bleiben nötig — der Redactor kennt den konkreten Key nicht,
/// und `redact_secrets` kennt kein Muster.
fn redact_for_debug(text: &str, secrets: &[&str]) -> String {
    default_log_redactor().redact_text(&redact_secrets(text, secrets))
}

/// Spec 0094, A1.2: Was von der Historie ab `info` stehen bleibt, ist
/// ausschließlich Art und Länge je Eintrag — der Text selbst
/// (Chat-Nachrichten, Notizen, Kommandotexte) steht nur noch auf `debug`,
/// s. [`history_contents`].
fn history_shapes(context: &SessionContext) -> Vec<String> {
    context
        .history
        .iter()
        .map(|m| match &m.content {
            MessageContent::Text(t) => {
                format!("[text] len={}", t.chars().count())
            }
            MessageContent::CommandResult {
                command,
                output,
                cancelled,
            } => format!(
                "[command_result] command_len={} exit={:?} stdout_len={} stderr_len={} \
                 cancelled={cancelled}",
                command.chars().count(),
                output.exit_code,
                output.stdout.len(),
                output.stderr.len()
            ),
            // Nur die Art der Ablehnung, nicht der Text von
            // `RejectionReason::Blocked` — A1.2 lässt „Art und Länge" zu.
            // Der Text stammt heute aus der Filter-Entscheidung (Regel-ID
            // bzw. fester Grund) und trüge damit kein Kommando; ihn hier
            // trotzdem weglassen kostet nichts und hält die Zeile
            // unabhängig davon, woraus dieser Grund künftig gebaut wird.
            MessageContent::ActionRejected { command, reason } => format!(
                "[action_rejected] command_len={} reason={}",
                command.chars().count(),
                match reason {
                    RejectionReason::User => "user",
                    RejectionReason::Blocked(_) => "blocked",
                    RejectionReason::Timeout => "timeout",
                }
            ),
        })
        .collect()
}

/// Der volle Inhalt je History-Eintrag — nur für die `debug`-Zeile (A2),
/// dort durch den Redactor gelaufen. Entspricht dem, was bis Spec 0094 auf
/// `info` stand.
fn history_contents(context: &SessionContext) -> Vec<String> {
    let redactor = default_log_redactor();
    context
        .history
        .iter()
        .map(|m| match &m.content {
            MessageContent::Text(t) => redactor.redact_text(t),
            MessageContent::CommandResult { command, output, cancelled } => format!(
                "[command_result] {} (exit={:?}, stdout_len={}, stderr_len={}, cancelled={cancelled})",
                redactor.redact_text(command),
                output.exit_code,
                output.stdout.len(),
                output.stderr.len()
            ),
            MessageContent::ActionRejected { command, reason } => format!(
                "[action_rejected] {} ({})",
                redactor.redact_text(command),
                match reason {
                    RejectionReason::User => "user".to_string(),
                    RejectionReason::Blocked(reason) => {
                        format!("blocked: {}", redactor.redact_text(reason))
                    }
                    RejectionReason::Timeout => "timeout".to_string(),
                }
            ),
        })
        .collect()
}

/// Spec 0016, Abschnitt 4, Punkt 1: der tatsächlich an den Provider
/// gesendete `SessionContext` — **nach** Redaction. Der hier ankommende
/// `context` wurde bereits in `app-shell::orchestration` redigiert, bevor
/// ein Kommando-Ergebnis überhaupt in `context.history` landete (s.
/// `OutputRedactor`, Spec 0006 Abschnitt 5) — diese Funktion loggt also nie
/// rohen, unredigierten Kommando-Output.
///
/// Spec 0094, A1.2/A2: Der frühere Aufbau — `system_context` und die volle
/// `history` auf `info` — ist aufgeteilt. Ab `info` steht nur noch die Form
/// (s. [`history_shapes`]), der Inhalt auf einer eigenen `debug`-Zeile
/// (s. [`history_contents`]). Die Begründung von Spec 0016, warum
/// `CommandResult` hier nur als Kommando + Längen erscheint und nicht als
/// voller Output (der steht pro Ausführung in
/// `app_logic::orchestration::action_exec`), gilt auf der `debug`-Zeile
/// unverändert weiter.
pub(crate) fn log_outgoing_context(request_id: Uuid, context: &SessionContext) {
    let action_names: Vec<&str> = context
        .available_actions
        .iter()
        .map(|a| a.name.as_str())
        .collect();

    // Spec 0094, A1.2: `system_context` (Systemprompt samt Servernotizen)
    // und `history` (der volle Chatverlauf, Notiztexte, Kommandotexte)
    // standen hier roh auf `info` — die umfangreichste Inhaltsquelle im
    // ganzen Log. Ab `info` bleiben `request_id`, die Anzahl der Einträge,
    // die Namen der Aktionen und je Eintrag nur Art und Länge.
    tracing::info!(
        request_id = %request_id,
        history_len = context.history.len(),
        history_shapes = ?history_shapes(context),
        available_actions = ?action_names,
        "outgoing session context to AI provider",
    );
    // Spec 0094, A2: derselbe Inhalt wie vorher, auf `debug` und redigiert.
    // `history_contents`/`redact_text` laufen nur, wenn diese Zeile
    // tatsächlich aufgezeichnet wird — `tracing`s Ereignis-Makros werten
    // ihre Feldausdrücke erst innerhalb des `if enabled`-Zweigs aus, den sie
    // selbst erzeugen.
    tracing::debug!(
        request_id = %request_id,
        system_context = %default_log_redactor().redact_text(&context.system_context),
        history = ?history_contents(context),
        "outgoing session context to AI provider (content)",
    );
}

/// Spec 0016, Abschnitt 4, Punkt 2: Text-Deltas zusammengefasst geloggt
/// (Gesamtlänge des Streams), nicht zeichenweise — die Spec erlaubt das
/// explizit ("reine Text-Deltas ggf. zusammengefasst statt Zeichen für
/// Zeichen").
pub(crate) fn log_text_delta_summary(request_id: Uuid, total_len: usize) {
    if total_len == 0 {
        return;
    }
    tracing::debug!(
        request_id = %request_id,
        text_len = total_len,
        "received text delta stream (summarized)",
    );
}

/// Spec 0080, A4: Am Ende jeder Runde des OpenAI-kompatiblen Providers
/// werden `text_len` (**auch bei 0**, anders als [`log_text_delta_summary`]
/// oben, das bei 0 ganz ausbleibt) und `reasoning_len` (Länge der
/// akkumulierten Denk-Deltas, `delta.reasoning_content`/`delta.reasoning`)
/// geloggt. Bewusst eine eigene Funktion statt [`log_text_delta_summary`]
/// zu erweitern: Spec 0080 §2 schließt Änderungen am Anthropic-Provider
/// aus, der Weiterhin `log_text_delta_summary` mit seinem bestehenden
/// "Skip bei 0"-Verhalten nutzt — genau der `text_len == 0`-Fall ist aber
/// der, den Spec 0080 sichtbar machen will (die leere, abgeschnittene
/// Runde aus §1, oft bei einem Reasoning-Modell, dessen Denk-Tokens das
/// Budget vor jedem Text aufbrauchen). Kein sensibler Inhalt: reine
/// Längenwerte, kein Text.
pub(crate) fn log_openai_round_summary(request_id: Uuid, text_len: usize, reasoning_len: usize) {
    tracing::info!(
        request_id = %request_id,
        text_len,
        reasoning_len,
        "AI response round ended (text/reasoning length)",
    );
}

/// Spec 0063, Teil 1: warum ein Turn endete — Anthropics `stop_reason`
/// (`message_delta`) bzw. eines OpenAI-kompatiblen Providers `finish_reason`
/// (`choices[].finish_reason`) wurden vorher nirgends geparst oder geloggt.
/// Ohne dieses Feld lässt sich ein beobachtetes "KI bricht mitten in der
/// Antwort ab, ohne das angekündigte Tool aufzurufen" nicht einordnen: `
/// end_turn`/`stop` (das Modell hat bewusst aufgehört) und `max_tokens`/
/// `length` (die Antwort wurde technisch abgeschnitten — ein Hinweis auf ein
/// zu niedriges Token-Limit, also ein echter Bug) sehen für den Nutzer
/// identisch aus ("die KI hört auf"), erfordern aber unterschiedliche
/// Reaktionen. Kein sensibler Inhalt: nur `request_id` + der rohe
/// Enum-/String-Wert des Providers.
///
/// `provider`: fester String je Aufrufer (`"anthropic"`/
/// `"openai_compatible"`) — spec-reviewer-Fund (Follow-up-Review): beide
/// Provider melden unter demselben Feldnamen `stop_reason`, ohne diese
/// Markierung ließe sich beim Log-Triagieren nicht erkennen, welches
/// Vokabular gilt (`end_turn` vs. `stop`).
pub(crate) fn log_stop_reason(request_id: Uuid, provider: &str, stop_reason: &str) {
    tracing::info!(
        request_id = %request_id,
        provider,
        stop_reason,
        "AI response turn ended",
    );
}

/// Spec 0064, Teil 5: macht die Prompt-Cache-Trefferquote sichtbar — ohne
/// dieses Logging weiß niemand, ob die Cache-Disziplin (Reihenfolge +
/// stabiles Präfix, s. Modul-Doc von `app_logic::compaction`) tatsächlich
/// greift oder ob irgendetwas das Präfix bricht. `usage` ist Anthropics
/// rohes `message.usage`-Objekt aus dem `message_start`-Event
/// (`cache_creation_input_tokens`/`cache_read_input_tokens`/
/// `input_tokens`/`output_tokens`) — hier NICHT einzeln typisiert
/// entgegengenommen, sondern als `&Value` durchgereicht und mit
/// `unwrap_or(0)` gelesen: fehlt ein Feld (älteres API-Verhalten, ein
/// Provider-Wechsel o. Ä.), loggt diese Funktion trotzdem eine vollständige
/// Zeile mit `0` statt gar nichts oder eines Parse-Fehlers. Kein sensibler
/// Inhalt: reine Zählwerte, kein Prompt-/Antwort-Text.
pub(crate) fn log_cache_usage(request_id: Uuid, provider: &str, usage: &Value) {
    let field = |name: &str| usage.get(name).and_then(Value::as_u64).unwrap_or(0);
    tracing::info!(
        request_id = %request_id,
        provider,
        cache_creation_input_tokens = field("cache_creation_input_tokens"),
        cache_read_input_tokens = field("cache_read_input_tokens"),
        input_tokens = field("input_tokens"),
        "AI request token usage (prompt cache visibility)",
    );
}

/// Spec 0016, Abschnitt 4, Punkt 2: ein vollständig akkumuliertes
/// Tool-Call-JSON-Fragment, sobald ein Block abgeschlossen ist — "vollständig"
/// bezieht sich auf den fertigen Block, nicht auf jedes einzelne
/// Zwischen-Chunk (die läppern sich oft zu keinem gültigen JSON für sich
/// genommen).
/// Spec 0094, A1.3: `raw_arguments` sind die Argumente des Werkzeugaufrufs
/// — in der Praxis der vorgeschlagene Kommandotext bzw. ein Notiz- oder
/// Dateiinhalt. Ab `info` bleibt davon nur die Länge.
pub(crate) fn log_tool_call_fragment(request_id: Uuid, tool_name: &str, raw_arguments: &str) {
    tracing::info!(
        request_id = %request_id,
        tool_name,
        raw_arguments_len = raw_arguments.chars().count(),
        "received tool call fragment",
    );
    tracing::debug!(
        request_id = %request_id,
        tool_name,
        raw_arguments = %default_log_redactor().redact_text(raw_arguments),
        "received tool call fragment (arguments)",
    );
}

/// Spec 0016, Abschnitt 4, Punkt 3, Erfolgsfall.
///
/// Spec 0094, A1.3: `action = ?action` trug hier das volle `AiAction` — bei
/// `SuggestCommand` das Kommando, bei `ProposeNoteUpdate`/`WriteRemoteFile`
/// den kompletten neuen Inhalt. Ab `info` bleibt nur
/// [`AiAction::kind`] („Art der Aktion").
pub(crate) fn log_tool_call_parsed(request_id: Uuid, action: &AiAction) {
    tracing::info!(
        request_id = %request_id,
        action_kind = action.kind(),
        "tool call parsed successfully",
    );
    tracing::debug!(
        request_id = %request_id,
        action_kind = action.kind(),
        action = %default_log_redactor().redact_text(&format!("{action:?}")),
        "tool call parsed successfully (action)",
    );
}

/// Spec 0016, Abschnitt 4, Punkt 3, Fehlerfall: die **vollständige
/// Rohantwort** plus die genaue Fehlermeldung — genau das, was im
/// beobachteten `target_id ist keine gültige UUID`-Bugfall (Spec 0016,
/// Abschnitt 1/6) gefehlt hätte, um sofort zu sehen, was die KI tatsächlich
/// geschickt hat.
///
/// Spec 0094, A1.3: Genau diese „vollständige Rohantwort" ist der Inhalt,
/// den A1 ab `error` verbietet — und der Fehlertext daneben ist es
/// ebenfalls. Er gibt bei einem `serde_json::Error` einen Ausschnitt der
/// Eingabe wieder und bei `AiError::InvalidResponse` den Argumentwert, der
/// die Validierung nicht bestand (z. B. „`target_id` ist keine gültige
/// UUID: <wert>"). Ab `error` bleiben deshalb nur Werkzeugname,
/// Argumentlänge und `error_code`; beides Rohe steht auf `debug`.
///
/// `error_code` kommt von der Aufrufstelle, weil sie den `AiError` kennt,
/// der aus diesem Fehlschlag tatsächlich entsteht — `error` selbst ist hier
/// nur `dyn Display` (bei kaputtem JSON ein `serde_json::Error`, das keinen
/// Code hat).
pub(crate) fn log_tool_call_parse_error(
    request_id: Uuid,
    tool_name: &str,
    raw_arguments: &str,
    error_code: &str,
    error: &dyn std::fmt::Display,
) {
    tracing::error!(
        request_id = %request_id,
        tool_name,
        raw_arguments_len = raw_arguments.chars().count(),
        code = error_code,
        "tool call parsing/validation failed",
    );
    tracing::debug!(
        request_id = %request_id,
        tool_name,
        code = error_code,
        raw_arguments = %default_log_redactor().redact_text(raw_arguments),
        error = %default_log_redactor().redact_text(&error.to_string()),
        "tool call parsing/validation failed (arguments and error text)",
    );
}

/// Spec 0049, Fund 2: bislang wurde nur die ausgehende Anfrage geloggt
/// (`log_outgoing_context` oben), nie die **Fehlerantwort** des Providers
/// — bei der Diagnose eines Tester-Problems musste geraten werden. Ergänzt
/// die verständliche UI-Meldung (Spec 0047, Fund D2), ersetzt sie nicht.
///
/// Für einen nicht-erfolgreichen HTTP-Status, **bevor** `body` von
/// `crate::error::map_http_status` auf eine der u. U. detailärmeren
/// `AiError`-Varianten (`AuthenticationFailed`/`RateLimited` sind Unit-
/// Varianten ohne Status/Body) reduziert wird — deshalb wird hier an den
/// eigentlichen Aufrufstellen geloggt (mit dem noch vollständigen
/// `status`/`body`), nicht erst am gemeinsamen App-Shell-Chokepoint, wo
/// diese Information für 401/429 bereits verloren wäre.
///
/// Redaction-Invariante (Spec 0016/gilt auch hier laut Spec 0049): `body`
/// ist strikt der Response-Body des Providers, nie die ausgehenden
/// Request-Header — der eigene API-Key kann dort unter normalen Umständen
/// gar nicht auftauchen. Trotzdem wird defensiv geprüft, ob einer der
/// `secrets` (API-Key, s. Aufrufstellen — bei `OpenAiCompatibleProvider`
/// zusätzlich jeder `extra_headers`-Wert, s. Spec 0025 Abschnitt 3: dort
/// landen in der Praxis zusätzliche Auth-Token, die ein Gateway/Proxy in
/// einer Fehlermeldung spiegeln könnte) wörtlich in `body` vorkommt und in
/// diesem Fall ersetzt — dieselbe "Logs sind kein Schlupfloch für
/// Secrets"-Regel wie bei
/// `app_logic::orchestration::action_exec::log_command_execution`.
///
/// Spec-Reviewer-Fund (Spec 0049, Review dieses Schritts): ursprünglich
/// nahm diese Funktion nur den API-Key entgegen — `extra_headers`-Werte
/// blieben ungeprüft.
pub(crate) fn log_provider_error_response(
    request_id: Uuid,
    status: u16,
    body: &str,
    error: &AiError,
    secrets: &[&str],
) {
    // Spec 0094, A1.5 — die **einzige** Ausnahme von A1: `body` bleibt auf
    // `warn`, weil eine Fehlkonfiguration (falsches Modell, falscher
    // Gateway-Pfad, abgelehnter Key) sich ohne die Antwort des Providers
    // nicht diagnostizieren lässt. Neu ist die Reihenfolge: **erst**
    // redigieren (wörtliche Secrets **und** Muster), **dann** kürzen — nie
    // umgekehrt, sonst schneidet die Kürzung ein Muster an und der Anfang
    // eines Geheimnisses bleibt stehen.
    let redacted = redact_for_debug(body, secrets);
    tracing::warn!(
        request_id = %request_id,
        status,
        code = error.code(),
        body = %truncate_logged_body(&redacted),
        "AI provider returned an error response",
    );
    // Spec 0094, A2: Was die Kürzung wegnimmt, steht auf `debug` — sonst
    // wäre bei einer langen Provider-Antwort genau die Information verloren,
    // für die diese Zeile existiert (Spec 0049, Fund 2).
    if redacted.chars().count() > MAX_LOGGED_BODY_LEN {
        tracing::debug!(
            request_id = %request_id,
            status,
            code = error.code(),
            body = %redacted,
            "AI provider returned an error response (full body)",
        );
    }
}

/// Spec 0051, Teil 1: ein HTTP 429, das automatisch mit Backoff
/// wiederholt wird — bewusst eine eigene Log-Zeile statt
/// [`log_provider_error_response`] wiederzuverwenden: Letztere markiert
/// einen *terminalen* Fehler (der Aufrufer bricht danach ab), während ein
/// 429 hier gerade *nicht* terminal ist. Dieselbe Redaction-Regel gilt
/// trotzdem (`body` könnte im Prinzip Header-Werte eines Proxys spiegeln,
/// s. [`log_provider_error_response`]-Doc-Kommentar).
pub(crate) fn log_provider_rate_limited_retry(
    request_id: Uuid,
    attempt: u32,
    body: &str,
    delay: std::time::Duration,
    secrets: &[&str],
) {
    // Spec 0094, A1.5: dieselbe Behandlung wie bei
    // [`log_provider_error_response`] — dieselbe Art Inhalt (Antwortkörper
    // des Providers), dieselbe Reihenfolge (redigieren, dann kürzen).
    let redacted = redact_for_debug(body, secrets);
    tracing::warn!(
        request_id = %request_id,
        attempt,
        delay_ms = delay.as_millis() as u64,
        body = %truncate_logged_body(&redacted),
        "AI provider rate-limited the request (429) — retrying with backoff",
    );
    if redacted.chars().count() > MAX_LOGGED_BODY_LEN {
        tracing::debug!(
            request_id = %request_id,
            attempt,
            body = %redacted,
            "AI provider rate-limited the request (429) — retrying with backoff (full body)",
        );
    }
}

/// Gegenstück zu [`log_provider_error_response`] für einen Transport-
/// Fehler (Verbindungsaufbau, Timeout, TLS, ...) — hier existiert kein
/// HTTP-Status/Response-Body, nur die über `Display` lesbare Fehlermeldung.
///
/// Spec-Reviewer-Fund (Spec 0049, Review dieses Schritts): der ursprüngliche
/// Doc-Kommentar behauptete, diese Meldung könne "nie den API-Key
/// enthalten" — das galt zwar für den API-Key selbst (der nie Teil einer
/// URL ist), aber `reqwest::Error`s `Display` hängt bei einem
/// Verbindungsfehler die Ziel-URL an, und `base_url` ist ein vom Nutzer
/// editierbares Feld (Spec 0049, Fund 1 trimmt es explizit) — ein Nutzer
/// könnte dort z. B. `https://user:token@proxy.intern/v1` eintragen, dessen
/// Userinfo dann unredigiert im Log gelandet wäre. Dieselben `secrets` wie
/// bei [`log_provider_error_response`] werden deshalb auch hier angewendet.
///
/// Spec 0094, A1.7: Das `Display` eines `AiError` kann Inhalt tragen
/// (`ModelNotFound` den vollen Antworttext des Providers, `InvalidResponse`
/// Argumentwerte) und stand hier über `message` auf `warn`. Ab `warn` bleibt
/// nur `code()` — das stand schon vorher daneben, das `error`-Feld fällt
/// also weg, statt ersetzt zu werden. Der Text steht auf `debug`.
pub(crate) fn log_provider_transport_error(request_id: Uuid, error: &AiError, secrets: &[&str]) {
    tracing::warn!(
        request_id = %request_id,
        code = error.code(),
        "AI provider transport/connection error",
    );
    tracing::debug!(
        request_id = %request_id,
        code = error.code(),
        error = %redact_for_debug(&error.to_string(), secrets),
        "AI provider transport/connection error (message)",
    );
}

/// Spec 0087, A1.6: eine erkannte Kontextgrenzen-400-Antwort wurde bereits
/// über [`log_provider_error_response`] mit Status + redigiertem Körper
/// geloggt (Spec 0049, Fund 2 — derselbe Pfad wie jeder andere
/// Providerfehler) — diese Funktion ergänzt nur die Debug-Zeile für den
/// tatsächlichen Retry-Schritt: altes und neues Budget, kein Körper (nichts
/// zu redigieren, reine Zahlen).
pub(crate) fn log_context_limit_retry(request_id: Uuid, old_max_tokens: u32, new_max_tokens: u32) {
    tracing::debug!(
        request_id = %request_id,
        old_max_tokens,
        new_max_tokens,
        "retrying with a smaller budget after a context-length error",
    );
}

/// Ersetzt jedes wörtliche Vorkommen jedes nicht-leeren Eintrags aus
/// `secrets` in `text` durch `[REDACTED]` (ein leerer Eintrag würde sonst
/// via `str::replace` jedes Zeichen "ersetzen").
fn redact_secrets(text: &str, secrets: &[&str]) -> String {
    let mut redacted = text.to_string();
    for secret in secrets {
        if !secret.is_empty() {
            redacted = redacted.replace(secret, "[REDACTED]");
        }
    }
    redacted
}

#[cfg(test)]
mod error_logging_tests {
    use super::*;
    use crate::test_support::{
        clear_log_buffer, debug_log_lines, install_test_subscriber_once, log_buffer_text,
        log_lines_at_info_or_above,
    };

    #[test]
    fn test_provider_error_response_logs_status_and_code_redacted() {
        install_test_subscriber_once();
        clear_log_buffer();

        let error = AiError::AuthenticationFailed;
        log_provider_error_response(
            Uuid::new_v4(),
            401,
            "invalid x-api-key",
            &error,
            &["sk-ant-real-secret-key"],
        );

        let log_text = log_buffer_text();
        assert!(log_text.contains("401"));
        assert!(log_text.contains("AI_AUTH_FAILED"));
        assert!(log_text.contains("invalid x-api-key"));
    }

    /// Der eigentliche Testfall aus der Spec: "ein simulierter Provider-401
    /// landet redigiert im Log (Key nicht)" — konstruiert absichtlich einen
    /// Body, der den API-Key wörtlich enthält (der worst case, den die
    /// Redaction abfangen soll, auch wenn ein echter Provider das nicht
    /// tut), und prüft, dass der Key selbst nirgends im Log-Output landet.
    #[test]
    fn test_provider_error_response_never_logs_the_api_key_verbatim() {
        install_test_subscriber_once();
        clear_log_buffer();

        let api_key = "sk-ant-real-secret-key";
        let error = AiError::AuthenticationFailed;
        log_provider_error_response(
            Uuid::new_v4(),
            401,
            &format!("invalid credentials for key {api_key}"),
            &error,
            &[api_key],
        );

        let log_text = log_buffer_text();
        assert!(
            !log_text.contains(api_key),
            "der API-Key darf unter keinen Umständen im Log-Output auftauchen: {log_text}"
        );
        assert!(
            log_text.contains("REDACTED"),
            "der Redaction-Platzhalter muss stattdessen im Log stehen: {log_text}"
        );
    }

    /// Spec-Reviewer-Fund (Spec 0049, Review dieses Schritts): `extra_headers`
    /// (Spec 0025, Abschnitt 3) können ein zweites Auth-Token tragen, nicht
    /// nur den API-Key — muss ebenfalls redigiert werden, wenn es als
    /// zusätzliches Secret übergeben wird.
    #[test]
    fn test_provider_error_response_redacts_every_given_secret_not_just_the_first() {
        install_test_subscriber_once();
        clear_log_buffer();

        let api_key = "sk-ant-real-secret-key";
        let extra_header_token = "gateway-token-xyz";
        let error = AiError::AuthenticationFailed;
        log_provider_error_response(
            Uuid::new_v4(),
            401,
            &format!("rejected: key={api_key} token={extra_header_token}"),
            &error,
            &[api_key, extra_header_token],
        );

        let log_text = log_buffer_text();
        assert!(
            !log_text.contains(api_key),
            "API-Key darf nicht im Log stehen: {log_text}"
        );
        assert!(
            !log_text.contains(extra_header_token),
            "extra_headers-Wert darf nicht im Log stehen: {log_text}"
        );
    }

    /// Spec 0094, A1.7: Der Code bleibt ab `warn`, die Fehlermeldung wandert
    /// auf `debug`. Vor Spec 0094 stand beides auf `warn`; dieser Test prüfte
    /// nur, dass beides *irgendwo* im Mitschnitt vorkommt, und hätte die
    /// Verschiebung nicht bemerkt — deshalb jetzt je Level getrennt.
    #[test]
    fn test_provider_transport_error_logs_code_at_warn_and_the_message_only_at_debug() {
        install_test_subscriber_once();
        clear_log_buffer();

        let error = AiError::NetworkError("connection refused".to_string());
        log_provider_transport_error(Uuid::new_v4(), &error, &[]);

        let info_or_above = log_lines_at_info_or_above();
        assert!(
            info_or_above.iter().any(|l| l.contains("AI_NETWORK_ERROR")),
            "der Fehlercode muss ab warn sichtbar bleiben: {info_or_above:?}"
        );
        assert!(
            !info_or_above
                .iter()
                .any(|l| l.contains("connection refused")),
            "die Fehlermeldung darf ab warn nicht mehr stehen (A1.7): {info_or_above:?}"
        );
        let debug_lines = debug_log_lines();
        assert!(
            debug_lines.iter().any(|l| l.contains("connection refused")),
            "auf debug muss die Meldung erhalten bleiben (A2): {debug_lines:?}"
        );
    }

    // --- Spec 0094: T4, T5, T8 ---------------------------------------------

    /// s. `ssh_manager_core::filter::tests::SECRET_0094` — bewusst eine Form,
    /// die der Redactor **nicht** erkennt, damit die Tests A1 prüfen und
    /// nicht die Redaction.
    const SECRET_0094: &str = "geheim-0094";

    fn secret_command() -> String {
        format!("mysql -p'{SECRET_0094}' -e 'select 1'")
    }

    /// Spec 0094, T4: Der ausgehende Kontext ist die umfangreichste
    /// Inhaltsquelle im Log — Systemprompt, Chatverlauf, Kommandotexte,
    /// Kommandoausgaben. Alle vier Sorten in einem Kontext, jede mit dem
    /// Geheimnis.
    #[test]
    fn test_t4_0094_outgoing_context_logs_no_content_at_info() {
        use ssh_manager_core::ai::{ActionSchema, ChatMessage, Role};
        use ssh_manager_core::ssh::CommandOutput;

        install_test_subscriber_once();
        clear_log_buffer();

        let context = SessionContext {
            system_context: format!("Serverhinweis: das Passwort ist {SECRET_0094}"),
            history: vec![
                ChatMessage {
                    role: Role::User,
                    content: MessageContent::Text(format!("bitte {} ausführen", secret_command())),
                },
                ChatMessage {
                    role: Role::ActionResult,
                    content: MessageContent::CommandResult {
                        command: secret_command(),
                        output: CommandOutput {
                            stdout: format!("ok, {SECRET_0094}\n").into_bytes(),
                            stderr: Vec::new(),
                            exit_code: Some(0),
                            truncated: false,
                        },
                        cancelled: false,
                    },
                },
                ChatMessage {
                    role: Role::ActionResult,
                    content: MessageContent::ActionRejected {
                        command: secret_command(),
                        reason: RejectionReason::Blocked(format!("Regel greift auf {SECRET_0094}")),
                    },
                },
            ],
            available_actions: vec![ActionSchema {
                name: "execute_command".to_string(),
                description: "führt ein Kommando aus".to_string(),
                parameters: Vec::new(),
            }],
            max_tokens_hint: None,
        };

        log_outgoing_context(Uuid::new_v4(), &context);

        let info_or_above = log_lines_at_info_or_above();
        assert!(
            !info_or_above.iter().any(|l| l.contains(SECRET_0094)),
            "weder system_context noch ein History-Eintrag darf ab info Inhalt tragen: \
             {info_or_above:?}"
        );
        assert!(
            !info_or_above.iter().any(|l| l.contains("mysql")),
            "auch der Kommandotext selbst darf ab info nicht stehen: {info_or_above:?}"
        );
        let line = info_or_above
            .iter()
            .find(|l| l.contains("outgoing session context to AI provider"))
            .expect("die Kontext-Zeile muss weiterhin ab info entstehen");
        assert!(
            line.contains("\"history_len\":3"),
            "die Anzahl der History-Einträge muss erhalten bleiben (A1.2): {line}"
        );
        assert!(
            line.contains("execute_command"),
            "die Namen der Aktionen müssen erhalten bleiben (A1.2): {line}"
        );
        for shape in [
            "[text] len=",
            "[command_result] command_len=",
            "exit=Some(0)",
        ] {
            assert!(
                line.contains(shape),
                "Art und Länge je Eintrag müssen erhalten bleiben (A1.2), fehlt {shape:?}: {line}"
            );
        }
        assert!(
            line.contains("[action_rejected] command_len=") && line.contains("reason=blocked"),
            "auch die abgelehnte Aktion braucht Art und Länge (A1.2): {line}"
        );

        // A2: auf debug steht der Inhalt weiterhin.
        let debug_lines = debug_log_lines();
        assert!(
            debug_lines.iter().any(|l| l
                .contains("outgoing session context to AI provider (content)")
                && l.contains("mysql")),
            "A2 verlangt eine debug-Zeile mit dem bisherigen Inhalt: {debug_lines:?}"
        );
    }

    /// Spec 0094, T5: Werkzeugaufruf in allen drei Formen — rohes Fragment,
    /// geparste Aktion, Parse-Fehler. Beim Parse-Fehler trägt der
    /// **Fehlertext** das Geheimnis (so wie `AiError::InvalidResponse` einen
    /// beanstandeten Argumentwert wiedergibt); ab `error` darf nur der Code
    /// bleiben.
    #[test]
    fn test_t5_0094_tool_call_logs_no_arguments_or_error_text_at_info() {
        install_test_subscriber_once();
        clear_log_buffer();

        let raw = format!(r#"{{"command":"{}"}}"#, secret_command());
        let request_id = Uuid::new_v4();
        log_tool_call_fragment(request_id, "execute_command", &raw);
        log_tool_call_parsed(
            request_id,
            &AiAction::SuggestCommand {
                command: secret_command(),
            },
        );
        let parse_error =
            AiError::InvalidResponse(format!("target ist kein bekannter Wert: {SECRET_0094}"));
        log_tool_call_parse_error(
            request_id,
            "execute_command",
            &raw,
            parse_error.code(),
            &parse_error,
        );

        let info_or_above = log_lines_at_info_or_above();
        assert!(
            !info_or_above.iter().any(|l| l.contains(SECRET_0094)),
            "weder Argumente noch Aktion noch Fehlertext dürfen ab info Inhalt tragen: \
             {info_or_above:?}"
        );
        assert!(
            !info_or_above.iter().any(|l| l.contains("mysql")),
            "der Kommandotext darf ab info nicht stehen: {info_or_above:?}"
        );
        assert!(
            info_or_above
                .iter()
                .any(|l| l.contains("\"code\":\"AI_INVALID_RESPONSE\"")),
            "beim Parse-Fehler muss der Fehlercode ab error stehen (A1.3): {info_or_above:?}"
        );
        assert!(
            info_or_above
                .iter()
                .any(|l| l.contains("\"action_kind\":\"suggest_command\"")),
            "die Art der Aktion muss erhalten bleiben (A1.3): {info_or_above:?}"
        );
        assert!(
            info_or_above
                .iter()
                .any(|l| l.contains("\"raw_arguments_len\":")),
            "die Länge der Argumente muss erhalten bleiben (A1.3): {info_or_above:?}"
        );

        let debug_lines = debug_log_lines();
        assert!(
            debug_lines.iter().any(|l| l.contains(SECRET_0094)),
            "A2 verlangt den bisherigen Inhalt auf debug: {debug_lines:?}"
        );
    }

    /// Spec 0094, T8a: A1.5 lässt den `body` auf `warn` stehen — dann muss
    /// aber der Redactor greifen. Das Geheimnis steht hier in einer Form, die
    /// er **kennt** (`--password=`), und bewusst in den ersten 100 Zeichen,
    /// also weit vor der Kürzungsgrenze: geprüft wird die Redaction, nicht
    /// dass die Kürzung das Geheimnis zufällig abschneidet.
    #[test]
    fn test_t8a_0094_provider_error_body_is_redacted_before_it_reaches_warn() {
        install_test_subscriber_once();
        clear_log_buffer();

        let body = format!(r#"{{"error":{{"message":"rejected --password={SECRET_0094}"}}}}"#);
        assert!(
            body.chars().count() < 100,
            "der Treffer muss vor der Kürzungsgrenze liegen"
        );
        let error = AiError::AuthenticationFailed;

        log_provider_error_response(Uuid::new_v4(), 401, &body, &error, &[]);

        let info_or_above = log_lines_at_info_or_above();
        assert!(
            !info_or_above.iter().any(|l| l.contains(SECRET_0094)),
            "der Redactor muss den body vor der warn-Zeile säubern: {info_or_above:?}"
        );
        assert!(
            info_or_above.iter().any(|l| l.contains("REDACTED")),
            "der Platzhalter muss stattdessen dort stehen: {info_or_above:?}"
        );
    }

    /// Spec 0094, T8b: Kürzung auf 512 Zeichen. Geprüft wird das `body`-Feld
    /// selbst, nicht die Zeilenlänge — die trägt noch Zeitstempel, Level und
    /// die übrigen Felder.
    #[test]
    fn test_t8b_0094_provider_error_body_is_truncated_to_512_characters_at_warn() {
        install_test_subscriber_once();
        clear_log_buffer();

        let body = "A".repeat(2000);
        let error = AiError::RateLimited;

        log_provider_error_response(Uuid::new_v4(), 429, &body, &error, &[]);

        let info_or_above = log_lines_at_info_or_above();
        let line = info_or_above
            .iter()
            .find(|l| l.contains("AI provider returned an error response"))
            .expect("die warn-Zeile muss entstehen");
        let parsed: Value = serde_json::from_str(line).expect("Log-Zeile ist JSON");
        let logged_body = parsed["fields"]["body"]
            .as_str()
            .expect("body-Feld muss ein String sein");
        let a_count = logged_body.chars().filter(|c| *c == 'A').count();
        assert_eq!(
            a_count, MAX_LOGGED_BODY_LEN,
            "A1.5: höchstens {MAX_LOGGED_BODY_LEN} Zeichen des body, war {a_count}"
        );
        assert!(
            debug_log_lines().iter().any(|l| l.contains("(full body)")),
            "A2: was die Kürzung wegnimmt, muss auf debug stehen"
        );
    }

    /// Spec 0094, T8c: `AiError::ModelNotFound` trägt den vollen Antworttext
    /// des Providers in seinem `Display` — geloggt über
    /// `log_provider_transport_error`, die A1.7-Stelle dieser Crate. Das
    /// Geheimnis steht hier in einer Form, die der Redactor **nicht** kennt,
    /// und es ist kein bekanntes `secrets`-Element: Nur weil A1.7 das Feld
    /// ganz entfernt, taucht es ab `warn` nicht auf.
    #[test]
    fn test_t8c_0094_model_not_found_display_never_reaches_warn() {
        install_test_subscriber_once();
        clear_log_buffer();

        let error = AiError::ModelNotFound(format!(
            r#"{{"error":{{"message":"unknown model, try sshpass -p {SECRET_0094}"}}}}"#
        ));

        log_provider_transport_error(Uuid::new_v4(), &error, &[]);

        let info_or_above = log_lines_at_info_or_above();
        assert!(
            !info_or_above.iter().any(|l| l.contains(SECRET_0094)),
            "das Display eines AiError darf ab warn nicht mehr geloggt werden (A1.7): \
             {info_or_above:?}"
        );
        assert!(
            info_or_above
                .iter()
                .any(|l| l.contains("AI_MODEL_NOT_FOUND")),
            "der Fehlercode muss ab warn bleiben (A1.7): {info_or_above:?}"
        );
    }

    /// Spec-Reviewer-Fund (Spec 0049, Review dieses Schritts): `reqwest`s
    /// `Display` für einen Transport-Fehler hängt die Ziel-URL an — steht
    /// dort (weil der Nutzer sie z. B. mit eingebetteter Proxy-Auth als
    /// `base_url` eingetragen hat) ein Secret drin, muss auch das hier
    /// redigiert werden, nicht nur beim HTTP-Fehler-Body.
    #[test]
    fn test_provider_transport_error_redacts_secret_embedded_in_the_message() {
        install_test_subscriber_once();
        clear_log_buffer();

        let embedded_secret = "user:proxy-token-123";
        let error = AiError::NetworkError(format!(
            "error sending request for url (https://{embedded_secret}@proxy.intern/v1)"
        ));
        log_provider_transport_error(Uuid::new_v4(), &error, &[embedded_secret]);

        let log_text = log_buffer_text();
        assert!(
            !log_text.contains(embedded_secret),
            "in der URL eingebettetes Secret darf nicht im Log stehen: {log_text}"
        );
    }

    /// Spec 0072, X1: ein Anbieter-Fehlertext ist nicht vertrauenswürdig —
    /// selbst wenn `map_http_status` daraus (strukturell, über
    /// `error.type == "not_found_error"`) ein `ModelNotFound` macht, dessen
    /// Wert den vollen Body wörtlich enthält (s. `crate::error`-Test
    /// `test_model_not_found_value_carries_the_full_body_for_downstream_
    /// redaction`), muss ein darin eingebettetes, als `secrets` bekanntes
    /// Secret weiterhin redigiert werden. Nutzt denselben Redaction-Pfad wie
    /// jeder andere `AiError` (`redact_secrets`, oben bereits durch
    /// `test_provider_error_response_never_logs_the_api_key_verbatim` u. a.
    /// belegt) — dieser Test bestätigt nur, dass der neue, strukturierte
    /// `ModelNotFound`-Fall über denselben Pfad läuft und keine Ausnahme
    /// davon ist, prüft **nicht** einen neuen Redaction-Mechanismus. Deckt
    /// bewusst nicht den Fall ab, dass der Provider-Text ein Secret enthält,
    /// das nicht in `secrets` steht — dafür gibt es auf diesem Pfad (Spec
    /// 0072 §5) keinen Muster-basierten Redactor.
    #[test]
    fn test_model_not_found_error_response_redacts_a_placeholder_secret_in_the_body() {
        install_test_subscriber_once();
        clear_log_buffer();

        let placeholder_secret = "sk-live-hunter2";
        let body = format!(
            r#"{{"error":{{"type":"not_found_error","message":"model: x (key {placeholder_secret})"}}}}"#
        );
        let error = crate::error::map_http_status(reqwest::StatusCode::NOT_FOUND, &body);
        assert!(matches!(error, AiError::ModelNotFound(_)));

        log_provider_error_response(Uuid::new_v4(), 404, &body, &error, &[placeholder_secret]);

        let log_text = log_buffer_text();
        assert!(
            !log_text.contains(placeholder_secret),
            "der Platzhalter darf in keiner geloggten Zeile auftauchen: {log_text}"
        );
        assert!(log_text.contains("AI_MODEL_NOT_FOUND"));
    }

    /// Spec 0063, Teil 1: `end_turn` (Modell hat bewusst aufgehört) und
    /// `max_tokens` (Antwort technisch abgeschnitten) müssen im Log klar
    /// unterscheidbar sein — sonst lässt sich ein "KI bricht mitten in der
    /// Antwort ab" nicht einordnen.
    #[test]
    fn test_log_stop_reason_distinguishes_end_turn_from_max_tokens() {
        install_test_subscriber_once();

        clear_log_buffer();
        log_stop_reason(Uuid::new_v4(), "anthropic", "end_turn");
        let log_text = log_buffer_text();
        assert!(log_text.contains("end_turn"));
        assert!(!log_text.contains("max_tokens"));

        clear_log_buffer();
        log_stop_reason(Uuid::new_v4(), "anthropic", "max_tokens");
        let log_text = log_buffer_text();
        assert!(log_text.contains("max_tokens"));
    }

    /// Spec 0080, A4: anders als [`log_text_delta_summary`] muss diese
    /// Funktion `text_len` AUCH BEI 0 loggen — genau der Fall, den Spec
    /// 0080 sichtbar machen will.
    #[test]
    fn test_log_openai_round_summary_logs_zero_text_len_and_reasoning_len() {
        install_test_subscriber_once();
        clear_log_buffer();

        log_openai_round_summary(Uuid::new_v4(), 0, 42);

        let log_text = log_buffer_text();
        assert!(
            log_text.contains("\"text_len\":0"),
            "text_len muss auch bei 0 geloggt werden: {log_text}"
        );
        assert!(
            log_text.contains("\"reasoning_len\":42"),
            "reasoning_len muss die Länge der Denk-Deltas tragen: {log_text}"
        );
    }
}
