//! Log lines for executed commands (Spec 0016 section 4 item 5, Spec 0094 A1.4/A2).

use ssh_manager_core::ai::OutputRedactor;
use ssh_manager_core::ssh::{CommandOutput, SshError};

use crate::state::SessionId;

/// Spec 0016, Abschnitt 4, Punkt 5: ab dieser Zeichenlänge wird geloggter
/// Kommando-Output gekürzt (mit Hinweis), statt den vollen — ggf. sehr
/// langen — Output in die Log-Datei zu schreiben. Der konfigurierbare
/// Knopf im Sinne der Spec ist diese Konstante selbst (analog zu
/// `core::filter::engine::DEFAULT_MAX_COMMAND_LENGTH`) — kein zur Laufzeit
/// änderbarer Wert, da dafür aktuell keine Einstellungs-UI existiert und
/// die Spec keine verlangt.
const MAX_LOGGED_OUTPUT_LEN: usize = 4096;

fn truncate_for_log(text: &str) -> String {
    if text.chars().count() <= MAX_LOGGED_OUTPUT_LEN {
        return text.to_string();
    }
    let truncated: String = text.chars().take(MAX_LOGGED_OUTPUT_LEN).collect();
    format!("{truncated}\n… (gekürzt, voller Output nicht geloggt)")
}

/// Spec 0016, Abschnitt 4, Punkt 5: Kommando, Exit-Code, Output-Länge.
/// Nimmt bewusst den bereits **redigierten** Output entgegen, nie den
/// rohen — Logs sind kein Schlupfloch für Secrets, die die Redaction sonst
/// unterdrückt (Spec 0016, Abschnitt 4, Punkt 1 — "dieselbe Redaction-Regel
/// gilt für Logs wie für den tatsächlichen API-Request").
///
/// Spec 0094, A1.4/A2: Redigiert reicht nicht — der Redactor kennt nicht
/// jede Schreibweise eines Passwort-Arguments (gemessen, §1.3), und
/// `stdout`/`stderr` können beliebigen Server-Inhalt tragen. Ab `info`
/// bleiben deshalb nur Zahlen: `session_id`, Exit-Code, Ausgabelängen,
/// Kommandolänge. Kommando und Ausgaben stehen auf `debug`, dort
/// unverändert redigiert und auf `MAX_LOGGED_OUTPUT_LEN` gekürzt.
///
/// Issue #143: `command_len` zählt Bytes (`str::len`) — dieselbe Einheit wie
/// `stdout_len`/`stderr_len`, das Längenlimit und das gleichnamige Feld der
/// Filter-Entscheidungszeile, damit beide Zeilen für dasselbe Kommando
/// dieselbe Zahl zeigen.
pub(super) fn log_command_execution(
    session_id: SessionId,
    command: &str,
    redacted_output: &CommandOutput,
) {
    let stdout = String::from_utf8_lossy(&redacted_output.stdout);
    let stderr = String::from_utf8_lossy(&redacted_output.stderr);
    tracing::info!(
        session_id = %session_id,
        command_len = command.len(),
        exit_code = ?redacted_output.exit_code,
        stdout_len = stdout.len(),
        stderr_len = stderr.len(),
        "ssh command executed",
    );
    // A2 verlangt, dass der Inhalt dieser Zeile „vorher durch den Redactor
    // läuft". Beides ist an der einzigen Aufrufstelle bereits redigiert
    // (`session.redactor`); hier läuft trotzdem noch einmal der
    // prozessweite Redactor darüber. Nicht, weil die Aufrufstelle
    // unzuverlässig wäre, sondern damit die Zusage dieser Funktion nicht an
    // einer Bedingung hängt, die ein künftiger zweiter Aufrufer übersehen
    // kann — Redaction ist idempotent, der Preis also nur Rechenzeit, und
    // die fällt nur an, wenn `debug` überhaupt aufgezeichnet wird.
    //
    // **Reihenfolge**: redigieren, dann kürzen — nie umgekehrt.
    // spec-reviewer-Fund (Runde 1): hier stand `redact_text(&truncate_for_
    // log(...))`. Genau die Reihenfolge, die die Spec unter
    // „Angriffsrichtungen" als Fehler benennt: die Kürzung bei Zeichen 4096
    // kann ein Secret-Muster entzweischneiden, sodass danach kein Muster
    // mehr greift und der Anfang des Geheimnisses im Klartext stehen bleibt.
    // Am heutigen Aufrufer folgenlos (der redigiert den vollen Text schon
    // vorher) — aber der zweite Durchlauf hier existiert gerade für den
    // Fall, dass ein künftiger Aufrufer das nicht tut, und für den war die
    // alte Reihenfolge falsch.
    tracing::debug!(
        session_id = %session_id,
        exit_code = ?redacted_output.exit_code,
        command = %ssh_manager_core::ai::default_log_redactor().redact_text(command),
        stdout = %truncate_for_log(
            &ssh_manager_core::ai::default_log_redactor().redact_text(&stdout),
        ),
        stderr = %truncate_for_log(
            &ssh_manager_core::ai::default_log_redactor().redact_text(&stderr),
        ),
        "ssh command executed (command and output)",
    );
}

/// Spec 0094, A1.4/A1.7: `command` war hier schon redigiert, `error = %err`
/// nicht — und `SshError`s `Display` gibt bei `ConnectionFailed`/
/// `ChannelError`/`CredentialResolutionFailed` freien Text der darunter
/// liegenden Bibliothek wieder, der das Kommando enthalten kann. Ab `warn`
/// bleiben Kommandolänge (in Bytes, Issue #143) und `code()`.
pub(super) fn log_command_execution_failed(session_id: SessionId, command: &str, err: &SshError) {
    tracing::warn!(
        session_id = %session_id,
        command_len = command.len(),
        code = err.code(),
        "ssh command execution failed",
    );
    // `default_log_redactor()` im Feldausdruck, nicht davor: so wird der
    // `OnceLock` (und damit das Übersetzen aller eingebauten Muster) im
    // Standardbetrieb ohne `debug` gar nicht erst angefasst
    // (spec-reviewer-Fund, Runde 1).
    tracing::debug!(
        session_id = %session_id,
        code = err.code(),
        command = %ssh_manager_core::ai::default_log_redactor().redact_text(command),
        error = %ssh_manager_core::ai::default_log_redactor().redact_text(&err.to_string()),
        "ssh command execution failed (command and error)",
    );
}
