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

/// Die fünf Plugins, die laut Klarstellung 9 im Passwort-Modus **erst nach
/// der Entsperrung** registriert werden: Sie berühren Dateien,
/// Einstellungen oder das Betriebssystem. Über `store` ist die
/// Einstellungsdatei lesbar, und ein altes `settings.json` kann noch den
/// MCP-Token tragen (A12) — das ist der Grund, warum diese Liste existiert.
const PLUGINS_DEFERRED_WHILE_LOCKED: &[&str] = &[
    "tauri_plugin_dialog::init()",
    "tauri_plugin_opener::init()",
    "tauri_plugin_store::Builder::new().build()",
    "tauri_plugin_os::init()",
    "tauri_plugin_notification::init()",
];

/// Der Rumpf einer Funktion in `lib.rs` als **Bereich** — von ihrem Kopf
/// bis zur ersten Zeile, die nur `}` enthält.
///
/// Ein Bereich und nicht der Text: Die Tests unten müssen fragen können, ob
/// eine Fundstelle *innerhalb* einer der beiden Funktionen liegt, und dafür
/// brauchen sie den Offset.
fn body_range(source: &str, signature: &str) -> std::ops::Range<usize> {
    let start = source
        .find(signature)
        .unwrap_or_else(|| panic!("`{signature}` gibt es nicht mehr — s. Doc-Kommentar"));
    let end = start
        + source[start..]
            .find("\n}\n")
            .expect("Funktionsende nicht gefunden — s. Doc-Kommentar");
    start..end
}

/// Klarstellung 10c: **Die Aufschiebung der Plugins hat einen eigenen
/// Test** — nach demselben Maßstab wie
/// [`test_t18_the_gate_is_wired_in_front_of_the_generated_dispatcher`].
///
/// Die Zusage aus Klarstellung 9 lautet: „A16 gilt für alle Kommandos, auch
/// die der Plugins." Sie hängt an einer einzigen Eigenschaft der
/// Builder-Kette — dass die fünf Plugins **nur** über
/// `register_unlocked_plugins_on` (Bauzeit, Schlüsselbund-Modus) und
/// `register_unlocked_plugins` (nach dem Entsperren) hineinkommen. Hinge
/// eines davon wieder unbedingt in der Kette, wäre sein Kommando vor der
/// Entsperrung erreichbar, und **kein** bestehender Test wäre rot geworden:
/// Das Tor sieht Plugin-Kommandos per Konstruktion nicht (gemessen M7, s.
/// `startup_gate`-Moduldoc).
///
/// Geprüft wird deshalb die Quelle, mit demselben bekannten Preis wie dort:
/// Wird ein Plugin ersetzt oder umbenannt, scheitert dieser Test, obwohl
/// nichts kaputt ist — dann gehört der neue Name in die Liste oben.
#[test]
fn test_the_deferred_plugins_are_registered_in_exactly_those_two_places() {
    let source = include_str!("lib.rs");
    let build_time_range = body_range(source, "fn register_unlocked_plugins_on(");
    let after_unlock_range = body_range(source, "pub(crate) fn register_unlocked_plugins(");
    let at_build_time = &source[build_time_range.clone()];
    let after_unlocking = &source[after_unlock_range.clone()];

    for plugin in PLUGINS_DEFERRED_WHILE_LOCKED {
        assert_eq!(
            source.matches(plugin).count(),
            2,
            "A16/Klarstellung 9+10c: `{plugin}` darf in `lib.rs` an genau zwei Stellen stehen — \
             in `register_unlocked_plugins_on` und in `register_unlocked_plugins`. Eine dritte \
             Stelle hängt es wieder unbedingt in die Kette und macht sein Kommando vor der \
             Entsperrung erreichbar."
        );
        assert_eq!(
            at_build_time.matches(plugin).count(),
            1,
            "`{plugin}` fehlt in `register_unlocked_plugins_on` — im Schlüsselbund-Modus wäre es \
             damit gar nicht registriert"
        );
        assert_eq!(
            after_unlocking.matches(plugin).count(),
            1,
            "`{plugin}` fehlt in `register_unlocked_plugins` — im Passwort-Modus bliebe es nach \
             dem Entsperren für die ganze Sitzung aus"
        );
    }

    // Und die Gegenrichtung: **jede** andere Registrierung in `lib.rs` ist
    // die eine, die bewusst auch im gesperrten Zustand gilt. Ohne diese
    // Aussage bliebe der Test grün, wenn jemand ein *neues* Plugin mit
    // Datei- oder Einstellungszugriff unbedingt in die Kette hängt.
    let elsewhere: Vec<&str> = source
        .match_indices(".plugin(")
        .filter(|(at, _)| !build_time_range.contains(at) && !after_unlock_range.contains(at))
        .map(|(at, _)| {
            let rest = &source[at..];
            // Bis zum Ende der Zeile — das genügt, um die Registrierung zu
            // benennen, und bleibt lesbar, wenn der Test scheitert.
            &rest[..rest.find('\n').unwrap_or(rest.len())]
        })
        .collect();
    assert_eq!(
        elsewhere,
        vec![".plugin(tauri_plugin_decoration::init())"],
        "A16: Außerhalb der beiden Funktionen darf nur das Plugin registriert werden, das den \
         Fensterrahmen gestaltet und keine Datei, Einstellung und keinen Zustand des \
         Betriebssystems liest. Kommt hier eines dazu, gehört es in \
         `register_unlocked_plugins_on`/`register_unlocked_plugins` — oder es braucht eine \
         eigene Begründung, und dieser Test die zweite Stelle."
    );
}

/// Klarstellung 10c, die **Richtung** der Aufschiebung: Aufgeschoben wird
/// genau dann, wenn beim Start **kein** Zustand steht — also im
/// Passwort-Modus vor der Entsperrung.
///
/// Eine umgekehrte Bedingung wäre das Gegenteil: Im Passwort-Modus hinge
/// alles in der Kette, und im Schlüsselbund-Modus fehlte es für immer. Der
/// Test oben würde das nicht sehen, weil beide Namen weiter genau einmal
/// vorkämen.
#[test]
fn test_the_plugins_are_deferred_exactly_when_the_app_starts_locked() {
    let source = include_str!("lib.rs");

    assert!(
        source.contains("let defer_plugins = app_state.is_none();"),
        "A16/Klarstellung 10c: Aufgeschoben wird **beim Fehlen** des Zustands. Erwartet ist \
         `let defer_plugins = app_state.is_none();`; eine andere Schreibweise kann dasselbe \
         meinen — dann gehört sie hier herein, geprüft."
    );

    let decision = {
        let start = source
            .find("let defer_plugins")
            .expect("die Entscheidung über die Aufschiebung gibt es nicht mehr");
        let rest = &source[start..];
        &rest[..rest
            .find("\n    builder\n")
            .expect("das Ende der Entscheidung ist nicht mehr zu finden")]
    };
    let defer_branch = {
        let start = decision
            .find("if defer_plugins {")
            .expect("die Verzweigung auf `defer_plugins` gibt es nicht mehr");
        let rest = &decision[start..];
        &rest[..rest.find("} else {").expect("kein `else`-Zweig mehr")]
    };

    assert!(
        !defer_branch.contains("register_unlocked_plugins"),
        "A16: Im aufgeschobenen Zweig darf nichts registriert werden — sonst ist die \
         Aufschiebung nur noch ein Log-Eintrag"
    );
    assert!(
        decision.contains("register_unlocked_plugins_on(builder)"),
        "A16: Im anderen Zweig müssen sie registriert werden, sonst fehlen sie im \
         Schlüsselbund-Modus dauerhaft"
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

    // **Die Richtung mitprüfen** (spec-reviewer Runde 6): „irgendwo steht
    // `try_state`, irgendwo danach `return`" bliebe auch bei einer
    // umgekehrten Bedingung grün (`if … .is_some() { return; }`) — und die
    // startete den Server genau dann, wenn die App gesperrt ist. Verlangt
    // ist deshalb die Form, die nur im Fehlen des Zustands zurückkehrt.
    assert!(
        body.contains("let Some(state) = app.try_state::<AppState>() else {"),
        "A16/T18: Der Wächter muss **beim Fehlen** des Zustands zurückkehren. Erwartet ist \
         `let Some(state) = app.try_state::<AppState>() else {{ … return; }}`; eine andere \
         Schreibweise kann dasselbe meinen — dann gehört sie hier herein, geprüft."
    );

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

/// A16, **zweite Hälfte** (ADR 0097 §4.3, erster der beiden fehlenden
/// Tests): Nach der Entsperrung gilt der Normalbetrieb — der Entsperrpfad
/// startet die Aufgaben nach dem Start.
///
/// Der Fund, gegen den dieser Test steht, war genau umgekehrt zu dem oben:
/// Dort darf der MCP-Server **nicht** laufen, solange die App gesperrt ist;
/// hier muss er nach dem Entsperren laufen. Ohne den Aufruf blieben
/// MCP-Autostart (Spec 0028 §9), das Aufräumen alter Sitzungen (0034) und
/// die Migration der Klartext-Zeilen (0040) im Passwort-Modus für die ganze
/// Sitzung aus — und kein Test wäre rot geworden: Der Doc-Kommentar von
/// `spawn_post_startup_tasks` behauptete den Aufruf, und es gab ihn nicht.
///
/// **Die Aussage, auf die es ankommt**, ist nicht „irgendwo in der Datei
/// steht der Aufruf", sondern: Er steht in derselben Funktion, die das Tor
/// öffnet — und das Tor wird nur dort geöffnet. Damit kann kein
/// Entsperrweg das Tor öffnen, ohne die Aufgaben zu starten.
///
/// Gelesen wird dafür die Quelle, mit demselben bekannten Preis wie bei den
/// Tests oben: Der ganze Pfad hängt an `tauri::AppHandle<Wry>` und ist mit
/// Tauris Test-Laufzeit nicht aufrufbar (ADR 0096 §3). Wird eine der beiden
/// Funktionen umbenannt, scheitert dieser Test, obwohl nichts kaputt ist —
/// dann gehört der neue Name hier herein.
#[test]
fn test_every_unlock_path_starts_the_post_startup_tasks() {
    let source = include_str!("commands/master_password.rs");
    let assembling = body_range(source, "async fn assemble_and_open_the_gate(");
    let body = &source[assembling.clone()];

    // Das Tor wird an genau einer Stelle geöffnet, und zwar hier. Ohne diese
    // Aussage bewiese der Rest nichts: Ein zweiter `gate.unlock()` an
    // anderer Stelle wäre ein Entsperrweg ohne Aufgaben.
    let unlock_sites: Vec<usize> = source
        .match_indices("gate.unlock()")
        .map(|(at, _)| at)
        .collect();
    assert_eq!(
        unlock_sites.len(),
        1,
        "A16: Das Tor darf in diesem Modul an genau einer Stelle geöffnet werden. Ist eine \
         zweite dazugekommen, braucht sie dieselben Schritte nach dem Entsperren — und dieser \
         Test die zweite Stelle."
    );
    assert!(
        assembling.contains(&unlock_sites[0]),
        "A16: Das Tor wird in `assemble_and_open_the_gate` geöffnet — dort stehen Zustand, Tor \
         und die Aufgaben nach dem Start in der einen Reihenfolge, auf die es ankommt"
    );

    let opens_the_gate = body
        .find("gate.unlock()")
        .expect("soeben geprüft, dass die Stelle in diesem Rumpf liegt");
    let starts_the_tasks = body.find("crate::spawn_post_startup_tasks(app)").expect(
        "A16, zweite Hälfte: Der Entsperrpfad muss `spawn_post_startup_tasks` aufrufen — sonst \
         laufen MCP-Autostart, Sitzungs-Aufräumen und die Migration der Klartext-Zeilen im \
         Passwort-Modus für die ganze Sitzung nicht",
    );
    assert!(
        opens_the_gate < starts_the_tasks,
        "A16: Erst das Tor, dann die Aufgaben — die Aufgaben brauchen Kommandos, die das Tor \
         vorher abweist"
    );
}
