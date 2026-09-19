//! Spec 0061: proaktive, provider-weite Rate-Limit-Drosselung.
//!
//! Vorher flog die App blind ins Rate-Limit — die `anthropic-ratelimit-*`-
//! Response-Header wurden nirgends gelesen, `map_http_status`
//! (`crate::error`) wirft Status/Header/Body ohnehin weg, sobald ein 429
//! einmal passiert ist (Unit-Variante `AiError::RateLimited`). Dieses Modul
//! liest die Header **vor** diesem Verlust (auf JEDER Antwort, nicht nur
//! 429 — Spec 0061 Abschnitt 1) und hält ein Restbudget pro
//! [`ProviderBudgetGuard`], damit `crate::anthropic`/`crate::
//! openai_compatible` **vor** dem nächsten Send proaktiv warten können,
//! statt erst reaktiv nach einem 429 zurückzurudern (Spec 0051, unverändert
//! als Sicherheitsnetz aktiv).
//!
//! Geteiltes Budget pro Provider-Identität (Spec 0061 Abschnitt 2, die
//! Kern-Entscheidung): [`provider_identity_key`] fasst `base_url`, `model`
//! und einen Hash des API-Keys zusammen — Anthropic limitiert pro
//! Organisation (praktisch: pro Key) UND trennt zusätzlich pro
//! Modell-Klasse. Ohne eine verlässliche Modell-Klassen-Tabelle wird der
//! exakte Modellname als konservative Näherung für "Modell-Klasse"
//! verwendet: im schlimmsten Fall entstehen dadurch zwei getrennte Wächter
//! für zwei Modelle, die intern dasselbe Kontingent teilen (verschenktes,
//! aber sicheres Wissen) — nie umgekehrt ein fälschlich geteiltes Budget.
//! [`RateLimitRegistry`] gibt für denselben Schlüssel immer denselben
//! [`ProviderBudgetGuard`] zurück (get-or-insert), lebt einmal pro App auf
//! `AppState` und wird von allen Aufruftypen (Haupt-Chat, Zweitmeinung,
//! Einschleusungs-Check, Zusammenfassung, Auto-Titel, Notiz-Vorschlag,
//! Notiz-Kürzung) geteilt, sobald sie denselben Key/Endpunkt/Modell nutzen.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Spec 0061, Entscheidung 1 (Stefan): konservativ ab ~15% Restbudget
/// bremsen (Puffer gegen Bursts — Anthropics Limiter reißt selbst bei noch
/// vorhandenem Budget kurzzeitig, wenn zu viele Anfragen gleichzeitig
/// eintreffen).
pub const THROTTLE_REMAINING_RATIO: f64 = 0.15;

/// Obergrenze für ein proaktives Warten. Invariante (Spec 0061, „kein
/// unbegrenztes Hängen"): fehlt der Reset-Zeitpunkt aus den Headern oder
/// ist er unsinnig (z. B. unparsbar, in der Vergangenheit), wird höchstens
/// so lange gewartet — danach übernimmt das bestehende reaktive Retry
/// (Spec 0051, `crate::retry`) als Sicherheitsnetz. Bewusst identisch zu
/// [`crate::sse::SSE_INACTIVITY_TIMEOUT`] gewählt: dieselbe Größenordnung,
/// mit der die App an anderer Stelle bereits "lange genug, aber nicht
/// ewig" für einen KI-Provider-bezogenen Wartevorgang definiert.
pub const MAX_PROACTIVE_WAIT: Duration = Duration::from_secs(90);

#[derive(Debug, Clone, Copy, Default)]
struct Counter {
    limit: Option<u64>,
    remaining: Option<u64>,
    reset_at: Option<Instant>,
    /// Wann diese Antwort (irgendeine, nicht nur eine mit neuen Werten für
    /// GENAU diesen Zähler) zuletzt verarbeitet wurde — s. [`Counter::
    /// is_stale`]. Follow-up-Fix (2. spec-reviewer-Runde) für eine
    /// Fail-open-Lücke im ursprünglichen `is_stale`: ohne diesen
    /// Zeitstempel wurde jeder Zähler mit verstrichenem `reset_at` als
    /// "veraltet, kein Warten" gewertet — auch wenn die GERADE
    /// eingetroffene Antwort ein frisches `remaining: 0` meldete, aber
    /// (z. B. wegen Uhren-Versatz) keinen brauchbaren `-reset`-Header
    /// mitschickte, sodass der alte, inzwischen verstrichene `reset_at`
    /// stehen blieb. Das drosselte dann gar nicht mehr, obwohl das
    /// Restbudget nachweislich 0 war.
    last_updated: Option<Instant>,
}

impl Counter {
    fn update(
        &mut self,
        now: Instant,
        limit: Option<u64>,
        remaining: Option<u64>,
        reset_at: Option<Instant>,
    ) {
        self.last_updated = Some(now);
        if limit.is_some() {
            self.limit = limit;
        }
        if remaining.is_some() {
            self.remaining = remaining;
        }
        if reset_at.is_some() {
            self.reset_at = reset_at;
        }
    }

    /// Wartedauer bis zum Reset (gedeckelt durch [`MAX_PROACTIVE_WAIT`]),
    /// für den Fall eines fehlenden Reset-Zeitpunkts. Wird nur aufgerufen,
    /// wenn `reset_at` entweder `None` ist oder (per [`Counter::is_stale`])
    /// bereits als "noch in der Zukunft" bestätigt wurde — ein bereits
    /// verstrichener Reset macht `limit`/`remaining` veraltet und wird VOR
    /// diesem Aufruf behandelt, nicht hier auf denselben 90s-Deckel
    /// abgebildet.
    fn capped_wait_until_reset(&self, now: Instant) -> Duration {
        match self.reset_at {
            Some(reset) if reset > now => (reset - now).min(MAX_PROACTIVE_WAIT),
            _ => MAX_PROACTIVE_WAIT,
        }
    }

    /// `true`, wenn dieser Zähler einen Reset-Zeitpunkt kennt, der bereits
    /// verstrichen ist — die zuletzt gelesenen `limit`/`remaining`-Werte
    /// stammen dann aus einem Fenster, das sich seit der letzten Antwort
    /// bereits wieder aufgefüllt hat, ohne dass eine neuere Antwort
    /// frischere Zahlen geliefert hätte. Von [`ratio_wait_duration`] UND dem
    /// Vorab-Schätzungs-Pfad in [`ProviderBudgetGuard::wait_duration`]
    /// genutzt, damit beide Pfade konsistent "keine verlässlichen Daten"
    /// statt eines unnötigen [`MAX_PROACTIVE_WAIT`]-Warten aus veralteten
    /// Zahlen ableiten (spec-reviewer Fund, Spec 0061 Follow-up).
    fn is_stale(&self, now: Instant) -> bool {
        match (self.reset_at, self.last_updated) {
            // Veraltet nur, wenn der Reset verstrichen ist UND seither
            // keine (auch nicht header-lose) Antwort mehr verarbeitet
            // wurde — kam gerade eine neue Antwort NACH dem Reset-
            // Zeitpunkt herein (auch ohne einen frischen `-reset`-Wert
            // selbst), sind `limit`/`remaining` aus dieser Antwort
            // weiterhin die aktuellsten bekannten Zahlen, kein
            // Alt-Zustand.
            (Some(reset), Some(last_updated)) => reset <= now && last_updated <= reset,
            (Some(reset), None) => reset <= now,
            (None, _) => false,
        }
    }

    /// Spec 0061 Abschnitt 3, erster Fall: dieser Zähler allein liegt unter
    /// der 15 %-Schwelle. `None`, wenn `limit`/`remaining` (noch) unbekannt
    /// sind, oder `limit == 0` (unsinniger Wert — wird ignoriert statt
    /// fälschlich als "0 % Restbudget" gewertet).
    ///
    /// Ein bereits verstrichener `reset_at` bedeutet: das Fenster hat sich
    /// seit der letzten gelesenen Antwort bereits aufgefüllt, aber es kam
    /// seither keine neue Antwort, die frischere Zahlen geliefert hätte —
    /// `limit`/`remaining` sind dann veraltet, nicht mehr "0,5 % Restbudget"
    /// wert. Das wird als "keine verlässlichen Daten" behandelt (kein
    /// Warten), NICHT wie ein fehlender Reset auf den vollen 90s-Deckel
    /// abgebildet — sonst wartet ein Nutzer, der eine Antwort in Ruhe liest
    /// und Minuten später antwortet, unnötig die volle Obergrenze, obwohl
    /// das TPM-Fenster längst wieder voll ist.
    fn ratio_wait_duration(&self, now: Instant) -> Option<Duration> {
        let (limit, remaining) = (self.limit?, self.remaining?);
        if limit == 0 {
            return None;
        }
        if self.is_stale(now) {
            return None;
        }
        let ratio = remaining as f64 / limit as f64;
        if ratio >= THROTTLE_REMAINING_RATIO {
            return None;
        }
        Some(self.capped_wait_until_reset(now))
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct BudgetState {
    requests: Counter,
    input_tokens: Counter,
    output_tokens: Counter,
    /// Manche Tarifstufen melden ein kombiniertes Tokens-Kontingent statt
    /// getrennter Input-/Output-Zähler (`anthropic-ratelimit-tokens-*`).
    tokens: Counter,
    /// Solange kein einziger Header je gelesen wurde (z. B. ein
    /// header-loser Provider wie Ollama, s. Modul-Doc), bleibt `false` —
    /// [`ProviderBudgetGuard::wait_duration`] gibt dann immer `None`
    /// zurück (Invariante: "ein header-loser Provider wird nie
    /// blockiert").
    has_any_header_data: bool,
}

/// Rohe, aus den Response-Headern EINER Antwort geparste Werte — pro
/// Provider-Typ eigens gebaut (s. `crate::anthropic::
/// parse_anthropic_rate_limit_headers`), damit ein anderer Provider mit
/// anderer/keiner Header-Konvention (Spec 0061 Abschnitt 1: "andere
/// Provider haben andere Header oder gar keine") diese Struktur einfach
/// leer lässt.
/// `pub` (nicht `pub(crate)`): neben den providereigenen Header-Parsern in
/// dieser Crate nutzt `app-shell`s Testsuite diese Struktur direkt, um
/// einen [`ProviderBudgetGuard`] ohne echten HTTP-Response gezielt in einen
/// bekannten Zustand zu versetzen (Spec 0061, Testbarkeit: "Budget unter
/// 15% → Gate wartet" lässt sich sonst nicht ohne echten Netzwerk-Mock auf
/// Crate-Grenze hinweg prüfen).
#[derive(Debug, Clone, Copy, Default)]
pub struct RateLimitHeaderSnapshot {
    pub requests: RawCounter,
    pub input_tokens: RawCounter,
    pub output_tokens: RawCounter,
    pub tokens: RawCounter,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RawCounter {
    pub limit: Option<u64>,
    pub remaining: Option<u64>,
    pub reset_at: Option<Instant>,
}

/// Hält das zuletzt aus Response-Headern bekannte Restbudget EINER
/// Provider-Identität (s. [`provider_identity_key`]). `Send + Sync` (nur
/// ein `Mutex` um reinen Werte-Zustand), beliebig oft über `Arc` geteilt.
#[derive(Debug, Default)]
pub struct ProviderBudgetGuard {
    state: Mutex<BudgetState>,
}

impl ProviderBudgetGuard {
    pub fn new() -> Self {
        Self::default()
    }

    /// Spec 0061 Abschnitt 1: wird auf JEDER Antwort aufgerufen (Erfolg
    /// **und** 429/andere Fehler), bevor `map_http_status`/`response.text()`
    /// die Header unerreichbar macht — s. Aufrufstellen in
    /// `crate::anthropic`/`crate::openai_compatible`. Aktualisiert nur die
    /// tatsächlich in `snapshot` vorhandenen Felder (ein Provider, der z. B.
    /// nur `-remaining`, aber (noch) kein `-reset` in dieser Antwort
    /// mitschickt, verliert den zuletzt bekannten Reset-Zeitpunkt nicht).
    pub fn record_headers(&self, snapshot: RateLimitHeaderSnapshot) {
        let now = Instant::now();
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.has_any_header_data = true;
        state.requests.update(
            now,
            snapshot.requests.limit,
            snapshot.requests.remaining,
            snapshot.requests.reset_at,
        );
        state.input_tokens.update(
            now,
            snapshot.input_tokens.limit,
            snapshot.input_tokens.remaining,
            snapshot.input_tokens.reset_at,
        );
        state.output_tokens.update(
            now,
            snapshot.output_tokens.limit,
            snapshot.output_tokens.remaining,
            snapshot.output_tokens.reset_at,
        );
        state.tokens.update(
            now,
            snapshot.tokens.limit,
            snapshot.tokens.remaining,
            snapshot.tokens.reset_at,
        );
    }

    /// Spec 0061 Abschnitt 3: Wartedauer vor dem nächsten `send()`, oder
    /// `None` für "sofort senden". Zwei unabhängige Gründe zu warten,
    /// beide werden geprüft, die LÄNGERE Wartezeit gewinnt:
    /// 1. irgendein Zähler liegt unter der 15 %-Schwelle
    ///    ([`Counter::ratio_wait_duration`]);
    /// 2. der vom Aufrufer geschätzte Input überschreitet das bekannte
    ///    Rest-Input-Token-Budget, UNABHÄNGIG von der 15 %-Schwelle (Spec
    ///    0061 Abschnitt 3, "Schätzung des Request-Gewichts" — verhindert,
    ///    dass ein einzelner großer Request ein knapp über der Schwelle
    ///    liegendes Budget trotzdem sprengt).
    ///
    /// Solange nie irgendein Header gelesen wurde, immer `None` (Invariante
    /// „header-loser Provider wird nie blockiert").
    pub fn wait_duration(&self, estimated_input_tokens: u64) -> Option<Duration> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if !state.has_any_header_data {
            return None;
        }
        let now = Instant::now();
        let mut longest: Option<Duration> = None;
        let mut consider = |wait: Option<Duration>| {
            if let Some(wait) = wait {
                longest = Some(longest.map_or(wait, |current: Duration| current.max(wait)));
            }
        };
        consider(state.requests.ratio_wait_duration(now));
        consider(state.input_tokens.ratio_wait_duration(now));
        consider(state.output_tokens.ratio_wait_duration(now));
        consider(state.tokens.ratio_wait_duration(now));

        if !state.input_tokens.is_stale(now) {
            if let Some(remaining) = state.input_tokens.remaining {
                if estimated_input_tokens > remaining {
                    consider(Some(state.input_tokens.capped_wait_until_reset(now)));
                }
            }
        }
        longest
    }
}

/// Spec 0061 Abschnitt 2: der Identitäts-Schlüssel, der bestimmt, welche
/// Aufrufe sich EIN Budget teilen. `base_url` (Endpunkt — deckt einen
/// unternehmensinternen Proxy mit demselben Key ab) + `model` (Anthropic
/// trennt Limits pro Modell-Klasse; der exakte Modellname ist eine
/// konservative Näherung dafür, s. Modul-Doc) + ein Hash des API-Keys
/// (steht als eigenständiger String im Registry-Schlüssel, statt den Key
/// selbst dort im Klartext zu duplizieren — `DefaultHasher` genügt, weil
/// hier keine kryptografische Stärke nötig ist, nicht weil er instabil
/// wäre: `DefaultHasher::new()` nutzt feste Schlüssel und ist damit über
/// Aufrufe UND Prozess-Neustarts hinweg stabil, s. ADR 0054 Abschnitt 1
/// für die Korrektur einer früheren, sachlich falschen Formulierung
/// dieser Doc-Zeile).
pub fn provider_identity_key(base_url: &str, model: &str, api_key: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    api_key.hash(&mut hasher);
    format!(
        "{}|{}|{:x}",
        base_url.trim_end_matches('/'),
        model,
        hasher.finish()
    )
}

/// Ein [`ProviderBudgetGuard`] pro Identität (s. [`provider_identity_key`]),
/// get-or-insert. Lebt einmal pro App-Prozess auf `AppState`
/// (`crate::state`-Äquivalent in `app-shell`, nicht Teil dieser Crate) —
/// `ai-providers` kennt kein `AppState`, stellt hier nur den Baustein
/// bereit.
#[derive(Debug, Default)]
pub struct RateLimitRegistry {
    guards: Mutex<HashMap<String, Arc<ProviderBudgetGuard>>>,
}

impl RateLimitRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn guard_for(&self, identity_key: &str) -> Arc<ProviderBudgetGuard> {
        let mut guards = self.guards.lock().unwrap_or_else(|e| e.into_inner());
        guards
            .entry(identity_key.to_string())
            .or_insert_with(|| Arc::new(ProviderBudgetGuard::new()))
            .clone()
    }
}

/// Liest einen `u64`-Header (z. B. `...-limit`/`...-remaining`) — `None` bei
/// fehlendem/unparsbarem Header, nie ein Panic (fail-safe: ein Provider mit
/// unerwartetem Header-Format degradiert zu "kein proaktives Wissen für
/// diesen Zähler", nicht zu einem Absturz).
fn header_u64(headers: &reqwest::header::HeaderMap, name: &str) -> Option<u64> {
    headers.get(name)?.to_str().ok()?.trim().parse().ok()
}

/// Liest einen `...-reset`-Header als RFC-3339-Zeitstempel (Anthropics
/// dokumentiertes Format) und rechnet ihn in eine monotone [`Instant`] um —
/// nötig, weil [`Counter::capped_wait_until_reset`] mit `Instant::now()`
/// vergleicht, nicht mit Wanduhrzeit (robust gegen Systemuhr-Sprünge
/// während der Wartezeit). Ein unparsbarer oder bereits verstrichener Wert
/// liefert `None` — `Counter::capped_wait_until_reset` fällt dann auf
/// [`MAX_PROACTIVE_WAIT`] zurück (Invariante „kein unbegrenztes Hängen bei
/// unsinnigem Reset").
fn header_reset_instant(headers: &reqwest::header::HeaderMap, name: &str) -> Option<Instant> {
    let raw = headers.get(name)?.to_str().ok()?.trim();
    let parsed = chrono::DateTime::parse_from_rfc3339(raw).ok()?;
    let delta_ms = (parsed.with_timezone(&chrono::Utc) - chrono::Utc::now()).num_milliseconds();
    if delta_ms <= 0 {
        return None;
    }
    // `checked_add` instead of `+`: a malformed/adversarial proxy could send
    // an absurdly-far-future reset timestamp, and `Instant + Duration`
    // panics on overflow rather than saturating. Treat that case the same
    // as "unparsable" (`None`) instead of crashing the provider task.
    Instant::now().checked_add(Duration::from_millis(delta_ms as u64))
}

/// Spec 0061 Abschnitt 1: Anthropic-spezifische Header-Namen. Bewusst nicht
/// generisch/providerübergreifend — jeder Provider-Typ kapselt seine
/// eigene Konvention (s. Modul-Doc).
pub(crate) fn parse_anthropic_rate_limit_headers(
    headers: &reqwest::header::HeaderMap,
) -> RateLimitHeaderSnapshot {
    let counter = |prefix: &str| RawCounter {
        limit: header_u64(headers, &format!("{prefix}-limit")),
        remaining: header_u64(headers, &format!("{prefix}-remaining")),
        reset_at: header_reset_instant(headers, &format!("{prefix}-reset")),
    };
    RateLimitHeaderSnapshot {
        requests: counter("anthropic-ratelimit-requests"),
        input_tokens: counter("anthropic-ratelimit-input-tokens"),
        output_tokens: counter("anthropic-ratelimit-output-tokens"),
        tokens: counter("anthropic-ratelimit-tokens"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue};

    fn header_map(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                reqwest::header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        map
    }

    fn future_rfc3339(seconds_from_now: i64) -> String {
        (chrono::Utc::now() + chrono::Duration::seconds(seconds_from_now)).to_rfc3339()
    }

    #[test]
    fn test_parse_anthropic_headers_extracts_all_four_counters() {
        let reset = future_rfc3339(30);
        let headers = header_map(&[
            ("anthropic-ratelimit-requests-limit", "50"),
            ("anthropic-ratelimit-requests-remaining", "40"),
            ("anthropic-ratelimit-requests-reset", &reset),
            ("anthropic-ratelimit-input-tokens-limit", "20000"),
            ("anthropic-ratelimit-input-tokens-remaining", "500"),
            ("anthropic-ratelimit-input-tokens-reset", &reset),
            ("anthropic-ratelimit-output-tokens-limit", "8000"),
            ("anthropic-ratelimit-output-tokens-remaining", "8000"),
            ("anthropic-ratelimit-tokens-limit", "28000"),
            ("anthropic-ratelimit-tokens-remaining", "8500"),
        ]);
        let snapshot = parse_anthropic_rate_limit_headers(&headers);
        assert_eq!(snapshot.requests.limit, Some(50));
        assert_eq!(snapshot.requests.remaining, Some(40));
        assert!(snapshot.requests.reset_at.is_some());
        assert_eq!(snapshot.input_tokens.remaining, Some(500));
        assert_eq!(snapshot.output_tokens.remaining, Some(8000));
        assert_eq!(snapshot.tokens.remaining, Some(8500));
    }

    #[test]
    fn test_guard_never_blocks_before_any_header_was_ever_recorded() {
        let guard = ProviderBudgetGuard::new();
        assert_eq!(guard.wait_duration(999_999), None);
    }

    #[test]
    fn test_guard_waits_when_a_counter_drops_below_the_threshold_ratio() {
        let guard = ProviderBudgetGuard::new();
        let reset = future_rfc3339(10);
        let headers = header_map(&[
            ("anthropic-ratelimit-input-tokens-limit", "1000"),
            ("anthropic-ratelimit-input-tokens-remaining", "100"),
            ("anthropic-ratelimit-input-tokens-reset", &reset),
        ]);
        guard.record_headers(parse_anthropic_rate_limit_headers(&headers));
        let wait = guard.wait_duration(10);
        assert!(
            wait.is_some(),
            "10% Restbudget muss unter der 15%-Schwelle bremsen"
        );
        assert!(wait.unwrap() <= MAX_PROACTIVE_WAIT);
    }

    #[test]
    fn test_guard_does_not_wait_when_all_counters_are_above_the_threshold() {
        let guard = ProviderBudgetGuard::new();
        let headers = header_map(&[
            ("anthropic-ratelimit-input-tokens-limit", "1000"),
            ("anthropic-ratelimit-input-tokens-remaining", "900"),
        ]);
        guard.record_headers(parse_anthropic_rate_limit_headers(&headers));
        assert_eq!(guard.wait_duration(10), None);
    }

    /// Spec 0061 Abschnitt 3: Vorab-Schätzung — ein großer geschätzter
    /// Input-Bedarf löst Warten aus, auch wenn die 15%-Schwelle für sich
    /// genommen noch nicht unterschritten ist.
    #[test]
    fn test_guard_waits_when_estimated_input_exceeds_remaining_budget_even_above_threshold() {
        let guard = ProviderBudgetGuard::new();
        let reset = future_rfc3339(5);
        let headers = header_map(&[
            ("anthropic-ratelimit-input-tokens-limit", "1000"),
            ("anthropic-ratelimit-input-tokens-remaining", "500"), // 50%, über der Schwelle
            ("anthropic-ratelimit-input-tokens-reset", &reset),
        ]);
        guard.record_headers(parse_anthropic_rate_limit_headers(&headers));
        assert_eq!(
            guard.wait_duration(200),
            None,
            "200 < 500 verbleibende Tokens, kein Warten nötig"
        );
        let wait = guard.wait_duration(800);
        assert!(
            wait.is_some(),
            "800 > 500 verbleibende Tokens muss vorab warten lassen"
        );
    }

    /// Regression für den spec-reviewer-Fund (Spec 0061 Follow-up):
    /// ein `reset_at`, der zwischen dem letzten Header-Lesen und dem
    /// jetzigen Gate-Check verstrichen ist, muss NICHT wie ein fehlender
    /// Reset auf den vollen [`MAX_PROACTIVE_WAIT`]-Deckel fallen — die
    /// Zähler-Daten sind dann veraltet, das TPM-Fenster hat sich längst
    /// wieder gefüllt. Konstruiert den Zustand direkt über die `pub`
    /// Testbarkeits-API (s. `RateLimitHeaderSnapshot`-Doc), weil
    /// `header_reset_instant` einen bereits verstrichenen Header-Wert schon
    /// beim Parsen zu `None` macht — das reproduziert nicht den Fall, dass
    /// ein früher gelesener, damals noch zukünftiger Reset inzwischen (real
    /// vergangene Zeit zwischen zwei Chat-Runden) verstrichen ist.
    #[test]
    fn test_guard_does_not_wait_the_full_cap_when_a_previously_future_reset_has_since_elapsed() {
        // Realistisches Szenario: die Header wurden vor 2 Minuten gelesen,
        // ihr `reset_at` war DAMALS noch 115s in der Zukunft, ist aber seit
        // 5s verstrichen — und seither kam keine neuere Antwort herein
        // (`last_updated` bleibt auf den Empfangszeitpunkt stehen). Direkter
        // Zugriff auf das interne `state`-Feld statt über `record_headers`
        // (das `last_updated` immer auf "jetzt" setzen würde) — nötig, um
        // genau diese Kombination nachzustellen, ohne 2 Minuten echte Zeit
        // verstreichen zu lassen; die Zweitmeinungs-Falle, die der
        // Follow-up-Fix (`Counter::last_updated`) schließt, betrifft exakt
        // den Fall "seit dem Empfang der Header ist keine neuere Antwort
        // mehr verarbeitet worden", s. `test_guard_still_waits_when_a_fresh_
        // response_reports_zero_remaining_without_a_usable_reset` unten für
        // die Gegenprobe.
        let guard = ProviderBudgetGuard::new();
        let now = Instant::now();
        let received_at = now - Duration::from_secs(120);
        let stale_reset = now - Duration::from_secs(5);
        {
            let mut state = guard.state.lock().unwrap();
            state.has_any_header_data = true;
            state.input_tokens = Counter {
                limit: Some(1000),
                remaining: Some(5), // 0.5%, weit unter der 15%-Schwelle
                reset_at: Some(stale_reset),
                last_updated: Some(received_at),
            };
        }
        assert_eq!(
            guard.wait_duration(10),
            None,
            "ein verstrichener Reset macht die Restbudget-Zahlen veraltet — \
             kein Warten auf Basis unbekannt gewordener Daten, statt fälschlich \
             MAX_PROACTIVE_WAIT wie bei einem fehlenden Reset"
        );
    }

    /// Gegenprobe zum Fix oben (2. spec-reviewer-Runde, Fail-open-Fund):
    /// kommt eine FRISCHE Antwort mit `remaining: 0` herein, deren
    /// `-reset`-Header fehlt/unparsbar/bereits verstrichen ist (sodass der
    /// alte, alte `reset_at` stehen bleibt), darf das NICHT als "veraltet,
    /// kein Warten" gewertet werden — diese Zahlen sind gerade erst
    /// eingetroffen, `last_updated` liegt NACH `reset_at`.
    #[test]
    fn test_guard_still_waits_when_a_fresh_response_reports_zero_remaining_without_a_usable_reset()
    {
        let guard = ProviderBudgetGuard::new();
        // Erste Antwort: normaler Zustand mit einem (damals) zukünftigen Reset.
        guard.record_headers(RateLimitHeaderSnapshot {
            input_tokens: RawCounter {
                limit: Some(1000),
                remaining: Some(500),
                reset_at: Some(Instant::now() + Duration::from_millis(10)),
            },
            ..Default::default()
        });
        // Der Reset-Zeitpunkt der ersten Antwort verstreicht real.
        std::thread::sleep(Duration::from_millis(30));
        // Zweite, FRISCHE Antwort: remaining fällt auf 0, aber ohne
        // brauchbaren `-reset`-Header (z. B. Uhren-Versatz/unparsbar) — der
        // alte, jetzt verstrichene `reset_at` bleibt unverändert stehen.
        guard.record_headers(RateLimitHeaderSnapshot {
            input_tokens: RawCounter {
                limit: Some(1000),
                remaining: Some(0),
                reset_at: None,
            },
            ..Default::default()
        });
        assert!(
            guard.wait_duration(10).is_some(),
            "eine GERADE eingetroffene Antwort mit remaining: 0 muss weiterhin drosseln, \
             auch wenn ihr eigener -reset-Header fehlt/unbrauchbar ist — sonst fährt die \
             App mit bekanntermaßen erschöpftem Budget blind weiter"
        );
    }

    #[test]
    fn test_guard_caps_wait_at_max_proactive_wait_when_reset_is_missing() {
        let guard = ProviderBudgetGuard::new();
        let headers = header_map(&[
            ("anthropic-ratelimit-requests-limit", "10"),
            ("anthropic-ratelimit-requests-remaining", "0"),
            // kein `-reset`-Header
        ]);
        guard.record_headers(parse_anthropic_rate_limit_headers(&headers));
        assert_eq!(guard.wait_duration(0), Some(MAX_PROACTIVE_WAIT));
    }

    #[test]
    fn test_guard_caps_wait_at_max_proactive_wait_when_reset_is_unparsable() {
        let guard = ProviderBudgetGuard::new();
        let headers = header_map(&[
            ("anthropic-ratelimit-requests-limit", "10"),
            ("anthropic-ratelimit-requests-remaining", "0"),
            ("anthropic-ratelimit-requests-reset", "not-a-timestamp"),
        ]);
        guard.record_headers(parse_anthropic_rate_limit_headers(&headers));
        assert_eq!(guard.wait_duration(0), Some(MAX_PROACTIVE_WAIT));
    }

    #[test]
    fn test_registry_returns_the_same_guard_for_the_same_identity() {
        let registry = RateLimitRegistry::new();
        let a = registry.guard_for("k1");
        let b = registry.guard_for("k1");
        assert!(Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn test_registry_returns_different_guards_for_different_identities() {
        let registry = RateLimitRegistry::new();
        let a = registry.guard_for("k1");
        let b = registry.guard_for("k2");
        assert!(!Arc::ptr_eq(&a, &b));
    }

    /// Spec 0061 Abschnitt 2: derselbe Key/Endpunkt/Modell -> derselbe
    /// Identitäts-Schlüssel, unabhängig vom Aufrufzweck (Haupt vs.
    /// Zweitmeinung) — das Registry-Verhalten allein reicht dafür, solange
    /// `provider_identity_key` für dieselben Eingaben denselben String
    /// liefert.
    #[test]
    fn test_identity_key_is_stable_and_distinguishes_key_base_url_and_model() {
        let a = provider_identity_key("https://api.anthropic.com", "claude-opus", "sk-1");
        let b = provider_identity_key("https://api.anthropic.com", "claude-opus", "sk-1");
        assert_eq!(a, b, "gleiche Eingaben müssen denselben Schlüssel liefern");

        let different_key =
            provider_identity_key("https://api.anthropic.com", "claude-opus", "sk-2");
        assert_ne!(a, different_key);

        let different_model =
            provider_identity_key("https://api.anthropic.com", "claude-sonnet", "sk-1");
        assert_ne!(a, different_model);

        let different_url = provider_identity_key("https://proxy.internal", "claude-opus", "sk-1");
        assert_ne!(a, different_url);

        // Nachlaufender Slash an der Base-URL darf keinen neuen Schlüssel
        // erzeugen (dieselbe Identität, nur kosmetisch anders geschrieben).
        let trailing_slash =
            provider_identity_key("https://api.anthropic.com/", "claude-opus", "sk-1");
        assert_eq!(a, trailing_slash);
    }

    #[test]
    fn test_identity_key_never_contains_the_raw_api_key() {
        let key = provider_identity_key(
            "https://api.anthropic.com",
            "claude-opus",
            "sk-super-secret-value",
        );
        assert!(!key.contains("sk-super-secret-value"));
    }
}
