//! 429-Retry mit Backoff (Spec 0051, Teil 1) — geteilt zwischen allen
//! Providern, analog zu `crate::error`/`crate::sse` (s. deren Moduldoc:
//! das Verhalten soll sich nicht zwischen `AnthropicProvider` und
//! `OpenAiCompatibleProvider` auseinanderentwickeln).
//!
//! **Redaction-Invariante (Spec 0051):** ein Retry wiederholt exakt den
//! bereits aufgebauten Request-Body unverändert (derselbe `body`-Wert wie
//! beim Erstversuch) — der Inhalt wurde bereits vor dem `AiProvider::
//! send()`-Aufruf in `app-shell::orchestration` redigiert
//! (`reapply_redaction_for_send`). Ein Retry fügt also nie neuen,
//! unredigierten Inhalt hinzu; er sendet nur denselben, bereits
//! redigierten Request ein weiteres Mal.

use std::time::Duration;

/// Harte Obergrenze an Versuchen (1 initialer Versuch + bis zu
/// `MAX_ATTEMPTS - 1` Wiederholungen bei HTTP 429) — Spec 0051, Teil 1:
/// "harte Obergrenze an Versuchen (z. B. 3–5)".
pub(crate) const MAX_ATTEMPTS: u32 = 4;

/// Gesamtzeit-Deckelung über alle Versuche/Wartezeiten hinweg (Spec 0051,
/// Teil 1: "Gesamtzeit-Deckelung") — verhindert, dass ein sehr langer
/// `Retry-After`-Wert (oder mehrere davon in Folge) den Chat-Turn
/// praktisch unbegrenzt blockiert, selbst wenn `MAX_ATTEMPTS` noch nicht
/// erreicht ist.
pub(crate) const MAX_TOTAL_RETRY_TIME: Duration = Duration::from_secs(60);

const BASE_BACKOFF: Duration = Duration::from_millis(500);
const MAX_BACKOFF: Duration = Duration::from_secs(20);

/// Liest `Retry-After` (RFC 9110, delta-seconds-Form). Die alternative
/// HTTP-date-Form wird bewusst nicht unterstützt: beide unterstützten
/// Provider-Familien (Anthropic, OpenAI-kompatibel) senden in der Praxis
/// ausschließlich die Sekunden-Form; eine Datums-Form nur für einen Fall
/// zu parsen, der bei keinem angebundenen Provider vorkommt, würde eine
/// weitere Abhängigkeit ohne beobachtbaren Nutzen hinzufügen. Ein
/// vorhandener, aber nicht als Sekunden-Zahl parsebarer Header fällt daher
/// auf das Backoff unten zurück statt den Retry abzubrechen.
pub(crate) fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let raw = headers.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?;
    let secs: u64 = raw.trim().parse().ok()?;
    Some(Duration::from_secs(secs))
}

/// Exponentielles Backoff mit Jitter für den Fall, dass kein (parsebarer)
/// `Retry-After`-Header vorliegt. `attempt` ist 1-basiert (der erste Retry
/// nach dem initialen Versuch ist `attempt == 1`). Der Jitter wird aus der
/// Systemzeit abgeleitet statt über eine zusätzliche `rand`-Abhängigkeit
/// bezogen — kein kryptografischer Anspruch, der Zweck ist nur, mehrere
/// zeitgleich zurückgewiesene Requests nicht exakt zeitgleich wieder
/// aufeinandertreffen zu lassen.
pub(crate) fn backoff_delay(attempt: u32) -> Duration {
    let exp = BASE_BACKOFF.saturating_mul(1u32 << attempt.min(6));
    let capped = exp.min(MAX_BACKOFF);
    let jitter_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_millis() % 250)
        .unwrap_or(0);
    capped + Duration::from_millis(u64::from(jitter_ms))
}

/// Wartezeit vor dem nächsten Retry-Versuch: `Retry-After` hat Vorrang vor
/// dem Backoff (Spec 0051, Teil 1: "`Retry-After` lesen und respektieren,
/// sonst exponentielles Backoff mit Jitter").
pub(crate) fn retry_delay(headers: &reqwest::header::HeaderMap, attempt: u32) -> Duration {
    parse_retry_after(headers).unwrap_or_else(|| backoff_delay(attempt))
}

/// Ob ein weiterer Retry-Versuch noch erlaubt ist: sowohl die
/// Versuchs-Obergrenze als auch die Gesamtzeit-Deckelung müssen das
/// zulassen (Spec 0051, Teil 1: beide Grenzen gelten unabhängig
/// voneinander, die zuerst erreichte gewinnt).
pub(crate) fn retry_allowed(next_attempt: u32, elapsed_since_start: Duration) -> bool {
    next_attempt <= MAX_ATTEMPTS && elapsed_since_start < MAX_TOTAL_RETRY_TIME
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_retry_after_reads_delta_seconds() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::RETRY_AFTER, "5".parse().unwrap());

        assert_eq!(parse_retry_after(&headers), Some(Duration::from_secs(5)));
    }

    #[test]
    fn test_parse_retry_after_none_when_header_missing() {
        let headers = reqwest::header::HeaderMap::new();

        assert_eq!(parse_retry_after(&headers), None);
    }

    #[test]
    fn test_parse_retry_after_falls_back_to_none_for_unparsable_value() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::RETRY_AFTER,
            "Wed, 21 Oct 2026 07:28:00 GMT".parse().unwrap(),
        );

        assert_eq!(parse_retry_after(&headers), None);
    }

    #[test]
    fn test_backoff_delay_grows_with_attempt_and_stays_capped() {
        let first = backoff_delay(1);
        let later = backoff_delay(5);

        assert!(first >= BASE_BACKOFF);
        assert!(later <= MAX_BACKOFF + Duration::from_millis(250));
        assert!(later >= first);
    }

    #[test]
    fn test_retry_allowed_respects_attempt_cap() {
        assert!(retry_allowed(MAX_ATTEMPTS, Duration::from_secs(0)));
        assert!(!retry_allowed(MAX_ATTEMPTS + 1, Duration::from_secs(0)));
    }

    #[test]
    fn test_retry_allowed_respects_total_time_cap() {
        assert!(!retry_allowed(2, MAX_TOTAL_RETRY_TIME));
        assert!(retry_allowed(
            2,
            MAX_TOTAL_RETRY_TIME - Duration::from_secs(1)
        ));
    }
}
