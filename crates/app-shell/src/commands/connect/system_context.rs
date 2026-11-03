//! Raw parts of the session system prompt (Spec 0057, Spec 0039).

use tauri::AppHandle;

use ssh_manager_core::filter::{EffectiveScope, EvalContext, PolicyStore};
use ssh_manager_core::profiles::{effective_notes_sections, ProfileStore};
use ssh_manager_core::shared::ServerId;

/// Gibt die Rohbestandteile des System-Prompts (Spec 0057, §4.1:
/// [`app_logic::compaction::SystemContextParts`] — getrennt gehalten statt
/// direkt zusammengefügt, damit die Kompaktierung Notiz-Sektionen einzeln
/// nach Scope priorisiert kürzen kann) zurück, zusammen mit der Info, ob
/// die Notizen nicht-leer sind (Spec 0039, Abschnitt 5) — der System-Prompt
/// wird bei **jeder** Nutzer-Nachricht neu gebaut und in jede KI-Anfrage
/// eingebettet; enthält er Notizen, ist damit ab diesem Zeitpunkt bereits
/// Inhalt aus einer nicht vertrauenswürdigen Quelle in den KI-Kontext
/// gelangt. Der Aufrufer nutzt das, um `Session::untrusted_content_
/// ingested` entsprechend zu setzen (monoton, s. dortiger Kommentar). Den
/// fertig zusammengesetzten String liefert `SystemContextParts::assemble`.
pub(crate) async fn build_session_system_context<R: tauri::Runtime>(
    app: &AppHandle<R>,
    server_name: &str,
    server_id: &ServerId,
    tags: &[String],
    profile_store: &dyn ProfileStore,
    policy_store: &persistence_sqlite::SqlitePolicyStore,
) -> (app_logic::compaction::SystemContextParts, bool) {
    // Spec 0032: der lokale Pseudo-Server hat keine `servers`-Zeile —
    // `profile_store.get_server` schlägt für ihn immer fehl, wodurch diese
    // Funktion sonst dauerhaft mit leeren Notizen liefe, obwohl über
    // `local_server::synthetic_server` tatsächlich welche hinterlegt sein
    // können (unabhängiger Review-Pass, s. docs/adr/0026).
    // `tracing::warn!` statt stillem `unwrap_or_default()`/leerem Fallback
    // (unabhängiger Review-Pass, Spec 0003/0004): ein Fehler hier bedeutet
    // nicht nur "keine Notizen geladen", sondern dass sicherheitsrelevanter
    // Kontext (z. B. "Produktionsserver, nur außerhalb des Wartungsfensters
    // anfassen") ohne jedes sichtbare Signal aus dem System-Prompt
    // verschwindet — die KI schlägt dann Kommandos vor, die sie mit
    // geladenen Notizen nicht vorschlagen würde.
    // Spec 0039, Abschnitt 3: unformatiert als (Quelle, Notiztext)-Paare
    // geladen statt als fertigen String — jeder Abschnitt muss einzeln
    // über `fence_untrusted` laufen, bevor er unten in den System-Prompt
    // eingebettet wird (der schwerwiegendste der vier in Spec 0039
    // genannten Befunde: Notizen persistieren über Sitzungen hinweg, eine
    // einmal eingeschleuste Anweisung wirkt also nicht nur einmalig).
    let note_sections: Vec<(String, String)> = if app_logic::dto::is_local(*server_id) {
        let local_notes = crate::local_server::synthetic_server(app).notes;
        if local_notes.trim().is_empty() {
            Vec::new()
        } else {
            vec![(format!("Server \"{server_name}\""), local_notes)]
        }
    } else {
        match profile_store.get_server(server_id).await {
            Ok(s) => match effective_notes_sections(&s, profile_store).await {
                Ok(sections) => sections,
                Err(err) => {
                    tracing::warn!(
                        server_id = %server_id.0,
                        error = %err,
                        "effective_notes_sections fehlgeschlagen — Session-Kontext enthält keine Notizen",
                    );
                    Vec::new()
                }
            },
            Err(err) => {
                tracing::warn!(
                    server_id = %server_id.0,
                    error = %err,
                    "get_server fehlgeschlagen — Session-Kontext enthält keine Notizen",
                );
                Vec::new()
            }
        }
    };

    // Issue #90: Sprache bei JEDEM Aufbau neu gelesen — ein Wechsel der
    // UI-Sprache gilt ab dem nächsten Aufbau, ohne Neustart.
    let language = crate::ui_language::session_prompt_language(app);
    let mut context = app_logic::system_prompt::base_prompt(language, server_name);

    let eval_ctx = EvalContext {
        server_id: *server_id,
        tags: tags.to_vec(),
    };
    let scope = EffectiveScope::from(&eval_ctx);
    let rules = policy_store.rules_for(&scope).await.into_trusted_rules();
    let allow_rules = app_logic::system_prompt::allow_rule_lines(&rules);

    context.push_str(&app_logic::system_prompt::allow_rules_section(
        language,
        &allow_rules,
    ));

    let parts = app_logic::compaction::SystemContextParts {
        base: context,
        note_sections,
        language,
    };
    let has_notes = parts.has_notes();
    (parts, has_notes)
}
