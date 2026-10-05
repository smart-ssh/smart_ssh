//! T18, zweite Hälfte (Spec 0101, A16): **Vor der Entsperrung ist kein
//! Kommando erreichbar, und der MCP-Server läuft nicht.**
//!
//! `startup_gate`s eigene Tests prüfen die Entscheidung
//! ([`StartupGate::allows`]) — also die Positivliste. Was dort nicht
//! vorkommt, ist alles, was **dahinter** steht: dass ein abgelehnter Aufruf
//! beim Aufrufer als Fehler mit dem Code aus A20 ankommt, dass der
//! Verteiler dabei gar nicht erst läuft, und dass das Tor überhaupt vor dem
//! von `generate_handler!` erzeugten Verteiler hängt. Genau das war die
//! Lücke: Eine Änderung, die `gated` aus der Builder-Kette nimmt, hätte
//! keinen Test rot gemacht.
//!
//! Deshalb hier der echte Weg mit Tauris Test-Laufzeit — Mock-App,
//! Webview, `get_ipc_response` — und ein Verteiler, der mitzählt, ob er
//! erreicht wurde.
//!
//! **Die ACL muss dafür ausdrücklich geöffnet werden** (gemessen): Die
//! Mock-App bringt eine leere ACL mit, und die liegt *vor* dem Tor (s.
//! `startup_gate`-Moduldoc). Ohne diesen Schritt beantwortet Tauri jeden
//! Aufruf mit „… not allowed. Plugin not found", und der Test prüfte nur
//! noch die ACL statt des Tors. `__allow_command` ist der dafür vorgesehene
//! Zugang (`#[doc(hidden)]`, aber öffentlich — Tauris eigene Tests nutzen
//! ihn); ein Aufruf davon im **Produktivcode** wäre ein Fehler, hier ist er
//! die Voraussetzung dafür, dass das Tor überhaupt gefragt wird.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
use tauri::webview::InvokeRequest;

use crate::startup_gate::{StartupGate, APP_LOCKED_CODE};

/// Was der Verteiler zurückgibt, wenn er erreicht wird. Steht in keiner
/// Fehlermeldung von Tauri und ist deshalb als „hier kamen Daten heraus"
/// eindeutig.
const DATA_FROM_THE_COMMAND: &str = "Daten-0101-aus-dem-Kommando";

/// Die Kommandos, die dieser Test durch den IPC-Weg schickt — alle in der
/// ACL der Mock-App erlaubt, damit allein das Tor entscheidet.
const COMMANDS_UNDER_TEST: &[&str] = &[
    "list_servers",
    "send_chat_message",
    "get_mcp_server_settings",
    "set_mcp_server_enabled",
    "get_app_info",
    "create_overlay_titlebar",
    "unlock_with_master_password",
    "quit_application",
    "start_over_from_unlock_screen",
    "get_startup_state",
];

struct GatedApp {
    app: tauri::App<MockRuntime>,
    reached: Arc<AtomicUsize>,
}

impl GatedApp {
    /// Eine Mock-App, deren `invoke_handler` **genau wie in `run()`** aus
    /// `gated(gate, …)` besteht. Der innere Verteiler steht für die
    /// `generate_handler!`-Liste: Er zählt seine Aufrufe und antwortet mit
    /// Daten.
    fn new(unlocked: bool) -> Self {
        let reached = Arc::new(AtomicUsize::new(0));
        let counter = reached.clone();
        let gate = Arc::new(StartupGate::new(unlocked));
        let mut context = mock_context(noop_assets());
        for command in COMMANDS_UNDER_TEST {
            context.runtime_authority_mut().__allow_command(
                (*command).to_string(),
                tauri::utils::acl::ExecutionContext::Local,
            );
        }
        let app = mock_builder()
            .invoke_handler(crate::gated(
                gate,
                move |invoke: tauri::ipc::Invoke<MockRuntime>| {
                    counter.fetch_add(1, Ordering::SeqCst);
                    invoke.resolver.resolve(DATA_FROM_THE_COMMAND);
                    true
                },
            ))
            .build(context)
            .expect("Mock-App konnte nicht gebaut werden");
        Self { app, reached }
    }

    fn webview(&self) -> tauri::WebviewWindow<MockRuntime> {
        tauri::WebviewWindowBuilder::new(&self.app, "main", tauri::WebviewUrl::default())
            .build()
            .expect("Webview konnte nicht gebaut werden")
    }

    /// Die Antwort als Text, wie sie beim Aufrufer ankommt: `Ok` der
    /// JSON-Rumpf des Verteilers, `Err` der Wert aus `reject`.
    fn call(
        &self,
        webview: &tauri::WebviewWindow<MockRuntime>,
        command: &str,
    ) -> Result<String, serde_json::Value> {
        let answer = tauri::test::get_ipc_response(
            webview,
            InvokeRequest {
                cmd: command.to_string(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                // `tauri://localhost` und nicht `http://tauri.localhost`:
                // Nur die erste Form gilt Tauri als **lokaler** Ursprung,
                // und nur dafür greift die oben erteilte Erlaubnis
                // (`ExecutionContext::Local`) — gemessen.
                url: "tauri://localhost".parse().expect("feste URL"),
                body: tauri::ipc::InvokeBody::default(),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_string(),
            },
        );
        answer.map(|body| match body {
            tauri::ipc::InvokeResponseBody::Json(text) => text,
            tauri::ipc::InvokeResponseBody::Raw(bytes) => {
                String::from_utf8_lossy(&bytes).to_string()
            }
        })
    }
}

/// T18: „Daten-Kommando und MCP-Anfrage im gesperrten Zustand → Fehler,
/// kein Panic, keine Daten." Alle drei Aussagen, über den echten IPC-Weg.
#[test]
fn test_t18_a_locked_app_answers_every_command_with_an_error_and_no_data() {
    let gated = GatedApp::new(false);
    let webview = gated.webview();

    for command in [
        // Daten-Kommandos.
        "list_servers",
        "send_chat_message",
        // Die MCP-Anfrage, soweit sie über ein Kommando geht: Ein- und
        // Ausschalten des Servers und das Lesen seiner Zugangsdaten.
        "get_mcp_server_settings",
        "set_mcp_server_enabled",
        // Ohne `AppState` im Argument — sie liefen ohne das Tor.
        "get_app_info",
        "create_overlay_titlebar",
    ] {
        let error = match gated.call(&webview, command) {
            Err(err) => err,
            Ok(body) => panic!("{command} hat Daten geliefert statt abzulehnen (A16): {body}"),
        };
        assert_eq!(
            error,
            serde_json::json!(APP_LOCKED_CODE),
            "{command} muss den Code aus A20 zurückgeben"
        );
        assert!(
            !error.to_string().contains(DATA_FROM_THE_COMMAND),
            "{command} darf keine Daten zurückgeben"
        );
    }

    assert_eq!(
        gated.reached.load(Ordering::SeqCst),
        0,
        "der Verteiler darf im gesperrten Zustand gar nicht erst laufen — \
         sonst hinge A16 daran, dass jedes einzelne Kommando selbst aufgibt"
    );
}

/// Die Gegenprobe, ohne die der Test oben auch mit einem Tor bestünde, das
/// **alles** sperrt: Entsperren, Beenden und „Neu anfangen" erreichen den
/// Verteiler, und zwar mit Daten.
#[test]
fn test_t18_the_named_choices_reach_the_dispatcher_while_locked() {
    let gated = GatedApp::new(false);
    let webview = gated.webview();

    for command in [
        "unlock_with_master_password",
        "quit_application",
        "start_over_from_unlock_screen",
        "get_startup_state",
    ] {
        let answer = gated
            .call(&webview, command)
            .unwrap_or_else(|err| panic!("{command} muss durchkommen (A16), kam aber: {err}"));
        assert_eq!(answer, serde_json::json!(DATA_FROM_THE_COMMAND).to_string());
    }

    assert_eq!(gated.reached.load(Ordering::SeqCst), 4);
}

/// Und der Normalbetrieb: Im Schlüsselbund-Modus steht das Tor offen, und
/// derselbe Aufruf, der oben abgelehnt wurde, liefert Daten.
#[test]
fn test_t18_the_keychain_mode_reaches_the_dispatcher() {
    let gated = GatedApp::new(true);
    let webview = gated.webview();

    let answer = gated
        .call(&webview, "list_servers")
        .expect("im Schlüsselbund-Modus gibt es nichts zu sperren");

    assert_eq!(answer, serde_json::json!(DATA_FROM_THE_COMMAND).to_string());
    assert_eq!(gated.reached.load(Ordering::SeqCst), 1);
}

/// Und die eine Aussage, die die drei Tests oben **nicht** treffen: Sie
/// bauen ihre eigene App um [`crate::gated`] und beweisen damit, was das Tor
/// tut — nicht, dass `run()` es benutzt. Nähme eine Änderung `gated` aus der
/// Builder-Kette, blieben sie grün.
///
/// Dafür wird hier die Quelle gelesen; zum Warum s. den Doc-Kommentar des
/// nächsten Tests (eine `Wry`-App lässt sich im Test nicht starten).
#[test]
fn test_t18_the_gate_is_wired_in_front_of_the_generated_dispatcher() {
    let source = include_str!("lib.rs");

    let sites: Vec<usize> = source
        .match_indices(".invoke_handler(")
        .map(|(at, _)| at)
        .collect();

    assert_eq!(
        sites.len(),
        1,
        "A16/T18: Es gibt genau einen Verteiler, und er ist die Stelle, an der das Tor hängt. \
         Ist hier ein zweiter dazugekommen, braucht er dasselbe Tor — und dieser Test die \
         zweite Stelle."
    );
    assert!(
        source[sites[0]..].starts_with(".invoke_handler(gated("),
        "A16/T18: Der Verteiler muss hinter `gated(…)` liegen, sonst ist vor der Entsperrung \
         jedes Kommando erreichbar"
    );
}

/// T18, der Teil über den **Server selbst**: Vor der Entsperrung startet er
/// nicht. Das hängt an zwei Stellen, und beide sind eine Bedingung auf einen
/// verwalteten `AppState` — die zweite ist die, auf die es ankommt:
/// [`crate::mcp_settings::autostart_if_enabled`] kehrt ohne ihn zurück,
/// **bevor** `start_server_if_not_running` erreichbar ist. Auch ein
/// verlorener Wächter an der Aufrufstelle in `run()` könnte den Server damit
/// nicht starten.
///
/// **Warum dieser Test die Quelle liest, statt die Funktion zu rufen:** Der
/// ganze Startpfad ist auf `tauri::Wry` festgelegt (`autostart_if_enabled`
/// → `start_server_if_not_running` → `AppMcpBackend`, das die Kommandos
/// aufruft). Mit Tauris Test-Laufzeit ist er nicht aufrufbar, und ein echter
/// `Wry`-Start braucht ein Fenstersystem. Die Alternativen wären gewesen,
/// die Kette für den Test über `R` zu abstrahieren (ein Umbau des
/// MCP-Backends, der mehr Risiko trägt als der Test Nutzen) oder den Punkt
/// weiter unbelegt zu lassen. Der Preis ist bekannt: Wird eines der beiden
/// Symbole umbenannt, scheitert dieser Test, obwohl nichts kaputt ist — dann
/// gehört der neue Name hier herein. Einzelheiten in ADR 0096.
#[test]
fn test_t18_the_mcp_autostart_gives_up_before_it_can_start_a_server() {
    let source = include_str!("mcp_settings.rs");

    let body = {
        let start = source
            .find("pub async fn autostart_if_enabled")
            .expect("autostart_if_enabled gibt es nicht mehr — s. Doc-Kommentar");
        let rest = &source[start..];
        let end = rest
            .find("\n}\n")
            .expect("Funktionsende nicht gefunden — s. Doc-Kommentar");
        &rest[..end]
    };

    let guard = body.find("try_state::<AppState>()").expect(
        "A16/T18: `autostart_if_enabled` muss ohne verwalteten `AppState` zurückkehren — \
         ohne diese Prüfung könnte der MCP-Server vor der Entsperrung starten",
    );
    let starts_the_server = body
        .find("start_server_if_not_running")
        .expect("der Server wird hier nicht mehr gestartet — s. Doc-Kommentar");

    assert!(
        guard < starts_the_server,
        "A16/T18: die Prüfung auf den verwalteten `AppState` muss **vor** dem Start des \
         MCP-Servers stehen"
    );
    assert!(
        body[guard..starts_the_server].contains("return"),
        "A16/T18: ohne verwalteten `AppState` muss die Funktion zurückkehren, nicht \
         weiterlaufen"
    );
}
