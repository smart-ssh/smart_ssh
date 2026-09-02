//! Optionale KI-Zweitmeinung für die Daten-Risiko-Achse (Spec 0026,
//! Abschnitt 3) — bewusst nur diese eine Achse, s. Spec-Begründung
//! ("semantisches Einordnen ... passt besser zu einer KI-Einschätzung als
//! Server-Schaden, der sich gut musterbasiert erfassen lässt").

use futures::StreamExt;
use tauri_plugin_store::StoreExt;

use ssh_manager_core::ai::{AiEvent, AiProvider, ChatMessage, MessageContent, Role, SessionContext};
use ssh_manager_core::risk::RiskLevel;

use crate::ai_provider_factory::build_ai_provider;
use crate::state::AppState;

/// Spec 0024, Abschnitt 4: derselbe `tauri-plugin-store`-Ablageort wie die
/// UI-Sprache (`frontend/src/i18n.ts`s `STORE_FILE`) — beide sind reine
/// UI-/App-Einstellungen ohne Bezug zu Server-/Gruppen-Fachdaten, keine
/// eigene SQLite-Migration nötig (Spec 0026, Abschnitt 1, Punkt 1 verlangt
/// das explizit: "keine neue SQLite-Tabelle").
const SETTINGS_STORE_FILE: &str = "settings.json";
const ENABLED_KEY: &str = "riskClassifierEnabled";
const PROVIDER_ID_KEY: &str = "riskClassifierProviderId";

/// Liest die Zweitmeinungs-Einstellungen und baut bei Bedarf den
/// konfigurierten `AiProvider` — einmalig bei `connect()` aufgerufen (s.
/// `Session::risk_second_opinion_provider`-Doc-Kommentar zur Begründung,
/// warum nicht live pro Aktionsvorschlag neu gelesen). `None`, wenn die
/// Zweitmeinung deaktiviert ist, kein Provider gewählt wurde, der gewählte
/// Provider inzwischen gelöscht wurde, oder sein Credential nicht auflösbar
/// ist — in jedem dieser Fälle bleibt Spec 0026 Abschnitt 3 Punkt 1 erfüllt
/// ("Standardmäßig deaktiviert"): lieber gar keine Zweitmeinung als eine
/// mit falscher/fehlender Konfiguration.
pub async fn resolve_second_opinion_provider(
    app: &tauri::AppHandle,
    state: &AppState,
) -> Option<Box<dyn AiProvider>> {
    let store = app.store(SETTINGS_STORE_FILE).ok()?;
    let enabled = store.get(ENABLED_KEY)?.as_bool().unwrap_or(false);
    if !enabled {
        return None;
    }
    let provider_id_raw = store.get(PROVIDER_ID_KEY)?.as_str()?.to_string();
    let provider_id = ssh_manager_core::ai::ProviderId(uuid::Uuid::parse_str(&provider_id_raw).ok()?);

    let config = state.ai_provider_store.get(&provider_id).await.ok()?;
    let api_key = state.credential_store.get(&config.credential_ref).ok()?;

    Some(build_ai_provider(
        config.provider_type,
        config.base_url.as_deref(),
        &config.model,
        api_key,
        config.supports_native_tool_calling,
        config.extra_headers.clone(),
    ))
}

/// Sinngemäß aus Spec 0026, Abschnitt 3 übernommen.
const SECOND_OPINION_PROMPT: &str = "Könnte die Ausgabe dieses Kommandos sensible Daten enthalten, \
     die nicht an einen KI-Anbieter weitergegeben werden sollten? Antworte nur mit none/yellow/red \
     und einer kurzen Begründung.";

/// Fragt `provider` nach einer Zweitmeinung zur Daten-Risiko-Achse für
/// `command_or_path` (Spec 0026, Abschnitt 3). **Minimaler Kontext**: nur
/// der Kommando-/Pfadtext selbst als einzige `history`-Nachricht, kein
/// Chatverlauf, keine Server-Notizen, keine `available_actions` (dasselbe
/// Sparsamkeitsprinzip wie beim `OutputRedactor`, Spec 0006 Abschnitt 5) —
/// `provider` ist typischerweise ein anderer, eigens für diesen Zweck
/// gewählter `AiProviderConfig` (Spec 0026, Abschnitt 3: "eigener, separat
/// wählbarer Provider"), nicht der Session-Provider.
///
/// `None`, wenn die Anfrage fehlschlägt ODER die Antwort sich nicht als
/// none/yellow/red erkennen lässt — "keine Zweitmeinung verfügbar" statt
/// eines Absturzes, im selben Geist wie das Fallback-Tool-Calling-Parsing
/// aus Spec 0006 (`ai_providers::fallback::parse_fallback_response`), das
/// bei nicht parsebarem Text ebenfalls graceful auf reinen Text zurückfällt
/// statt einen Fehler zu erzeugen.
pub async fn fetch_second_opinion(
    provider: &dyn AiProvider,
    command_or_path: &str,
) -> Option<(RiskLevel, String)> {
    let context = SessionContext {
        system_context: SECOND_OPINION_PROMPT.to_string(),
        history: vec![ChatMessage {
            role: Role::User,
            content: MessageContent::Text(command_or_path.to_string()),
        }],
        available_actions: Vec::new(),
    };

    let mut stream = provider.send(context);
    let mut text = String::new();
    while let Some(event) = stream.next().await {
        match event {
            AiEvent::TextDelta(delta) => text.push_str(&delta),
            AiEvent::Done => break,
            // Netzwerk-/Auth-/sonstiger Providerfehler: keine Zweitmeinung
            // verfügbar, kein Absturz, kein Blockieren der (bereits
            // angezeigten) regelbasierten Einschätzung.
            AiEvent::Error(_) => return None,
            // Nicht erwartet (leere `available_actions`), aber auch kein
            // Fehlerfall — ein Provider ohne natives Tool-Calling könnte
            // theoretisch trotzdem einen Fallback-Aktionsblock parsen,
            // falls der Prompt-Text zufällig danach aussieht. Einfach
            // ignoriert, das Textergebnis zählt.
            AiEvent::ActionProposed(_) => {}
        }
    }

    parse_second_opinion(&text)
}

/// Sucht das erste Wort, das (nach Entfernen von Satzzeichen,
/// case-insensitiv) exakt `none`/`yellow`/`red` entspricht — ein reiner
/// Teilstring-Suche (`text.contains("red")`) würde z. B. auf "redirect"
/// fehltriggern. Alles nach diesem Wort wird als Begründung übernommen;
/// bleibt danach nichts Sinnvolles übrig, wird stattdessen die volle
/// Antwort als Begründung verwendet (besser eine unbeschnittene Antwort
/// zeigen als eine leere Begründung).
fn parse_second_opinion(text: &str) -> Option<(RiskLevel, String)> {
    let words: Vec<&str> = text.split_whitespace().collect();
    for (i, word) in words.iter().enumerate() {
        let cleaned: String = word
            .chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect();
        let level = match cleaned.as_str() {
            "none" => RiskLevel::None,
            "yellow" => RiskLevel::Yellow,
            "red" => RiskLevel::Red,
            _ => continue,
        };

        let rest = words[i + 1..].join(" ");
        let rest_trimmed = rest.trim_start_matches(|c: char| !c.is_alphanumeric()).trim();
        let reason = if rest_trimmed.is_empty() {
            text.trim().to_string()
        } else {
            rest_trimmed.to_string()
        };
        return Some((level, reason));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_second_opinion_recognizes_none() {
        let (level, reason) = parse_second_opinion("none - looks like an ordinary read").unwrap();
        assert_eq!(level, RiskLevel::None);
        assert_eq!(reason, "looks like an ordinary read");
    }

    #[test]
    fn test_parse_second_opinion_recognizes_yellow() {
        let (level, reason) = parse_second_opinion("yellow: could contain internal hostnames").unwrap();
        assert_eq!(level, RiskLevel::Yellow);
        assert_eq!(reason, "could contain internal hostnames");
    }

    #[test]
    fn test_parse_second_opinion_recognizes_red() {
        let (level, reason) = parse_second_opinion("red, this looks like a private key dump").unwrap();
        assert_eq!(level, RiskLevel::Red);
        assert_eq!(reason, "this looks like a private key dump");
    }

    #[test]
    fn test_parse_second_opinion_does_not_false_trigger_on_substring() {
        // "redirect" enthält "red" als Teilstring, ist aber nicht das
        // erwartete Schlüsselwort — ein reiner `contains`-Check würde das
        // fälschlich als "red" lesen.
        let result = parse_second_opinion("this command just redirects output to a file");
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_second_opinion_unparseable_text_yields_none() {
        assert_eq!(parse_second_opinion("I'm not sure, hard to say"), None);
    }

    #[test]
    fn test_parse_second_opinion_keyword_without_trailing_reason_uses_full_text() {
        let (level, reason) = parse_second_opinion("red").unwrap();
        assert_eq!(level, RiskLevel::Red);
        assert_eq!(reason, "red");
    }
}
