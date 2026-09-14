//! Kompaktierung des an den KI-Provider gesendeten Kontexts (Spec 0057,
//! §3 + §4.1) — Etappe 2 des "reichen Session-Modells": Token-Schätzung
//! (Post-Fencing) + der Kompaktierungs-Auslöser, zunächst mit **einfacher
//! Kürzung** (Runden abschneiden ohne Zusammenfassung, s. Spec 0057 §8,
//! Etappe 2 — die KI-Zusammenfassung selbst kommt erst in Etappe 3 und
//! ersetzt dann den hier eingefügten Platzhalter-Hinweis).
//!
//! **Ersetzt** das bisherige `chat_context_truncation`-Modul (Spec 0034,
//! Abschnitt 9): jenes kürzte nur nach einem festen, providerunabhängigen
//! 40.000-Zeichen-Budget (s. ADR 0029) — unterschätzte dabei systematisch,
//! weil es rohe Zeichen zählte, ohne die Fencing-Expansion (Spec 0039:
//! `<`/`>`/`&` werden zu 4-5-Zeichen-Entitäten) zu berücksichtigen, und
//! kannte weder das tatsächliche Kontextfenster des konfigurierten
//! Modells noch eine Priorisierung nach "Runden"/Notiz-Scope — genau der
//! in Spec 0057 §3.1 dokumentierte Diagnose-Fund. S. ADR 0048 für die
//! Ablösungs-Begründung.
//!
//! Kürzt (wie schon `chat_context_truncation`) ausschließlich die an
//! [`ssh_manager_core::ai::AiProvider::send`] übergebene *Kopie* des
//! Kontexts — weder `Session::context` (die im Frontend angezeigte,
//! vollständige Historie) noch die gespeicherte Notiz noch das Ledger
//! (Spec 0057, §3.3/§6, Etappe 1) werden je verändert, s.
//! [`compact_for_send`]-Doc-Kommentar.

use ssh_manager_core::ai::{
    fence_untrusted, ActionSchema, ChatMessage, MessageContent, ProviderType, RejectionReason,
    Role, SessionContext, UntrustedKind,
};

/* --------------------- Token-Schätzung (Spec 0057, §3.1) ------------------- */

/// Grobe, providerunabhängige Heuristik: ~4 Byte pro Token — die
/// Standard-Faustregel für überwiegend ASCII-/lateinisch-lastigen Text
/// (Shell-Output, Notizen, Chat-Text), wie er hier weit überwiegt. Bewusst
/// KEIN echter Tokenizer pro Modell: unterschiedliche Provider/Modelle
/// verwenden unterschiedliche Tokenizer, ein exakter Nachbau für jeden
/// wäre unverhältnismäßiger Aufwand für eine Größe, die ohnehin nur einen
/// PROAKTIVEN Sicherheitsabstand vor dem harten Limit braucht (s.
/// [`COMPACTION_TRIGGER_RATIO`]) — dieselbe Heuristik-statt-Tokenizer-
/// Abwägung wie beim (hiermit abgelösten) `chat_context_truncation::
/// DEFAULT_CHAR_BUDGET` (ADR 0029), nur nicht mehr als fixe Zeichenzahl,
/// sondern als Prozentsatz des tatsächlichen, modellabhängigen
/// Kontextfensters ausgedrückt (s. [`model_context_window_tokens`]) — das
/// behebt den in Spec 0057 §3.1 genannten Diagnose-Fund (rohe Zeichen
/// unterschätzen systematisch), ohne die grundsätzliche Entscheidung aus
/// ADR 0029 zu verwerfen.
const BYTES_PER_TOKEN_ESTIMATE: usize = 4;

fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(BYTES_PER_TOKEN_ESTIMATE)
}

/// Grobe Schätzung des Text-Overheads der festen Wrapper-Tags, die
/// `ai_providers::{anthropic,openai_compatible}::format_command_result`
/// um `stdout`/`stderr` legt (`<command_execution_result>`, `<exit_code>`,
/// `<security_notice>`, ggf. `<cancelled_by_user>`/`<output_truncated>`) —
/// jene Funktion ist providerintern und wird nicht exportiert (s. ADR
/// 0048); eine Konstante statt exakter Nachbildung reicht für eine
/// KONSERVATIVE (eher zu hohe als zu niedrige) Schätzung.
const COMMAND_RESULT_WRAPPER_TOKEN_OVERHEAD: usize = 100;

/// Wie [`COMMAND_RESULT_WRAPPER_TOKEN_OVERHEAD`], für
/// `format_action_rejected` (deutlich kleinerer, fester Tag-Rahmen).
const ACTION_REJECTED_WRAPPER_TOKEN_OVERHEAD: usize = 20;

/// Schätzt die Post-Fencing-Größe einer einzelnen Nachricht (Spec 0057,
/// §3.1: "muss das Post-Fencing-Volumen bewerten"). `MessageContent::Text`
/// trägt in dieser Codebasis entweder gewöhnlichen KI-/Nutzer-Text (nie
/// gefenct) oder bereits VOR dem Ablegen in der Historie fertig gefencten
/// Inhalt (z. B. `execute_read_remote_file`s `<remote_file>`-Block, Spec
/// 0039) — in beiden Fällen ist der gespeicherte String bereits die
/// tatsächlich zu sendende Form, ein einfaches `estimate_tokens` genügt.
/// `MessageContent::CommandResult` dagegen wird ERST beim eigentlichen
/// Request-Aufbau in `ai-providers` gefenced (`fence_untrusted` läuft dort
/// über `stdout`/`stderr`) — hier wird dieselbe Funktion mit denselben
/// Parametern aufgerufen, um das tatsächliche Post-Fencing-Volumen zu
/// messen statt es nachzubilden/zu schätzen.
fn estimate_message_tokens(message: &ChatMessage) -> usize {
    match &message.content {
        MessageContent::Text(text) => estimate_tokens(text),
        MessageContent::CommandResult {
            command, output, ..
        } => {
            let stdout_fenced = fence_untrusted(
                UntrustedKind::CommandStdout,
                command,
                &String::from_utf8_lossy(&output.stdout),
            );
            let stderr_fenced = fence_untrusted(
                UntrustedKind::CommandStderr,
                command,
                &String::from_utf8_lossy(&output.stderr),
            );
            estimate_tokens(command)
                + estimate_tokens(&stdout_fenced)
                + estimate_tokens(&stderr_fenced)
                + COMMAND_RESULT_WRAPPER_TOKEN_OVERHEAD
        }
        MessageContent::ActionRejected { command, reason } => {
            let reason_text = match reason {
                RejectionReason::User => "",
                RejectionReason::Timeout => "",
                RejectionReason::Blocked(reason) => reason.as_str(),
            };
            estimate_tokens(command)
                + estimate_tokens(reason_text)
                + ACTION_REJECTED_WRAPPER_TOKEN_OVERHEAD
        }
    }
}

fn estimate_action_schemas_tokens(actions: &[ActionSchema]) -> usize {
    actions
        .iter()
        .map(|action| {
            let mut tokens = estimate_tokens(&action.name) + estimate_tokens(&action.description);
            for param in &action.parameters {
                tokens += estimate_tokens(&param.name) + estimate_tokens(&param.description);
            }
            tokens
        })
        .sum()
}

/// Schätzt die Gesamtgröße (in Token) dessen, was `context` tatsächlich an
/// den Provider senden würde — `system_context` (bereits fertig gefenct,
/// s. [`SystemContextParts::assemble`]) + Historie (Post-Fencing je
/// Nachricht, s. [`estimate_message_tokens`]) + die angebotenen
/// Werkzeug-Schemas (ändern sich nicht durch Kompaktierung, zählen aber
/// zur tatsächlichen Anfragegröße dazu — sonst würde die Schätzung
/// systematisch unterschätzen).
pub(crate) fn estimate_request_tokens(context: &SessionContext) -> usize {
    estimate_tokens(&context.system_context)
        + context
            .history
            .iter()
            .map(estimate_message_tokens)
            .sum::<usize>()
        + estimate_action_schemas_tokens(&context.available_actions)
}

/* -------------- Modell-Kontextfenster (Spec 0057, §3.1) -------------- */

/// Konservativer Default für unbekannte/generische Modelle
/// (`GenericOpenAiCompatible`/`Ollama`, oder ein unbekannter Modellname
/// bei einem bekannten Provider) — bewusst klein gewählt: lieber etwas zu
/// früh kompaktieren als bei einem tatsächlich kleinen Kontextfenster
/// (typische lokale/Ollama-Standardeinstellungen liegen oft nur bei
/// 2k-8k Token) zu spät. Die "sichere" Fehlrichtung ist immer "zu klein
/// annehmen", nie "zu groß" — ein zu großzügig angenommenes Fenster ist
/// genau der Fall, der den in Spec 0057 diagnostizierten Hänger
/// verursacht hat.
const DEFAULT_CONTEXT_WINDOW_TOKENS: usize = 32_000;

/// Bestimmt das geschätzte Kontextfenster (in Token) des konfigurierten
/// Modells (Spec 0057, §3.1). Keine echte Modell-Registry (es gibt aktuell
/// keine Persistenz/Pflege für so etwas — `AiProviderConfig` trägt nur
/// einen freien `model: String`, s. ADR 0048) — eine kleine, bewusst NICHT
/// erschöpfende Namens-Heuristik für die aktuell gängigsten Modelle, mit
/// konservativem Fallback (s. [`DEFAULT_CONTEXT_WINDOW_TOKENS`]) für alles
/// Unbekannte/für selbstgehostete Modelle (`GenericOpenAiCompatible`/
/// `Ollama`), deren tatsächliches Kontextfenster von der lokalen
/// Konfiguration abhängt und von hier aus prinzipiell nicht bekannt sein
/// kann.
pub(crate) fn model_context_window_tokens(provider_type: ProviderType, model: &str) -> usize {
    let model = model.to_lowercase();
    match provider_type {
        // Alle aktuellen Claude-Modelle (Stand dieser Implementierung)
        // teilen sich ein 200k-Token-Kontextfenster — kein Bedarf für eine
        // Namens-Fallunterscheidung innerhalb von Anthropic.
        ProviderType::Anthropic => 200_000,
        ProviderType::OpenAi => {
            if model.contains("gpt-3.5") {
                16_000
            } else {
                // gpt-4o/gpt-4-turbo/gpt-4.1/o1/o3 u. a. — aktuell
                // durchweg mindestens 128k.
                128_000
            }
        }
        ProviderType::GenericOpenAiCompatible | ProviderType::Ollama => {
            DEFAULT_CONTEXT_WINDOW_TOKENS
        }
    }
}

/* --------------------- Der Auslöser (Spec 0057, §3.1) --------------------- */

/// Ab wie viel Prozent des geschätzten Modell-Kontextfensters kompaktiert
/// wird, BEVOR das harte Limit erreicht wird (Spec 0057, §3.1: "~70-80%,
/// Puffer für die Antwort lassen") — der verbleibende Rest (hier 25%) IST
/// der Puffer für die Antwort, kein separater Abzug nötig. Benannte,
/// leicht justierbare Konstante (Spec-Vorgabe: "als benannte Konstante,
/// leicht justierbar").
const COMPACTION_TRIGGER_RATIO: f64 = 0.75;

/// Letzte N Runden, die Schritt 1 der Kürzungs-Reihenfolge IMMER
/// vollständig erhält (Spec 0057, §3.2, Punkt 1: "z. B. 3").
const MIN_PRESERVED_ROUNDS: usize = 3;

/* --------------- Schritt 1: alte Runden kürzen (Spec 0057, §3.2.1) -------- */

/// Eine "Runde" beginnt bei jeder `Role::User`-Nachricht und umfasst alle
/// folgenden Nachrichten bis zur nächsten `Role::User`-Nachricht (die vom
/// Nutzer ausgelöste Anfrage plus alle KI-Antworten/Aktionsergebnisse, die
/// daraus folgten) — dieselbe informelle Bedeutung wie anderswo im Code
/// ("automatische Folgerunde", `orchestration::MAX_AUTO_FOLLOWUP_ROUNDS`).
/// Nachrichten VOR der ersten `Role::User`-Nachricht (in der Praxis nicht
/// erreichbar, da jede Historie mit einer Nutzer-Nachricht beginnt, s.
/// `commands::send_chat_message_impl`) bilden defensiv eine eigene, erste
/// Gruppe, statt verlorenzugehen.
fn split_into_rounds(history: Vec<ChatMessage>) -> Vec<Vec<ChatMessage>> {
    let mut rounds: Vec<Vec<ChatMessage>> = Vec::new();
    for message in history {
        if matches!(message.role, Role::User) || rounds.is_empty() {
            rounds.push(vec![message]);
        } else {
            rounds
                .last_mut()
                .expect("mindestens eine Runde existiert bereits, s. Bedingung oben")
                .push(message);
        }
    }
    rounds
}

/// Spec 0057, §3.2, Schritt 1: alte Runden werden auf einen einzelnen
/// Platzhalter-Hinweis gekürzt, die letzten `min_preserved_rounds` bleiben
/// vollständig erhalten. Reines Abschneiden — noch OHNE Zusammenfassung
/// (Spec 0057, §8: "Etappe 2 ... einfache Kürzung", die KI-Summary ersetzt
/// diesen Platzhalter erst in Etappe 3). No-op, wenn ohnehin nicht mehr
/// als `min_preserved_rounds` Runden vorhanden sind. Kürzt UNBEDINGT auf
/// genau `min_preserved_rounds` — anders als [`compact_rounds_for_budget`]
/// (die tatsächlich in [`compact_for_send`] verwendete, budgetbewusste
/// Variante) ohne Rücksicht darauf, ob weniger Kürzung schon gereicht
/// hätte. Als eigenständiger, einfacher Baustein erhalten (u. a. direkt
/// getestet).
#[cfg(test)]
fn truncate_rounds_with_placeholder(
    history: Vec<ChatMessage>,
    min_preserved_rounds: usize,
) -> Vec<ChatMessage> {
    let rounds = split_into_rounds(history);
    if rounds.len() <= min_preserved_rounds {
        return rounds.into_iter().flatten().collect();
    }

    let cut_count = rounds.len() - min_preserved_rounds;
    let mut result = vec![round_truncation_placeholder(cut_count)];
    for round in rounds.into_iter().skip(cut_count) {
        result.extend(round);
    }
    result
}

/// Wie [`truncate_rounds_with_placeholder`], aber budgetbewusst
/// (spec-reviewer-Fund, Review dieses Schritts: die Aufgabenstellung
/// verlangt "nur so weit wie nötig" — nicht sofort auf
/// `min_preserved_rounds` kürzen, sondern Runde für Runde entfernen und
/// nach jeder Entfernung prüfen, ob der Request schon wieder unters
/// Budget passt). Belässt `context.history` unverändert, wenn schon
/// unterhalb `min_preserved_rounds` Runden vorhanden sind — dann kann
/// Schritt 1 ohnehin nichts mehr beitragen.
fn compact_rounds_for_budget(
    context: &mut SessionContext,
    min_preserved_rounds: usize,
    budget_tokens: usize,
) {
    let rounds = split_into_rounds(std::mem::take(&mut context.history));
    if rounds.len() <= min_preserved_rounds {
        context.history = rounds.into_iter().flatten().collect();
        return;
    }

    // Fixe Anteile (System-Kontext, Werkzeug-Schemas) ändern sich in
    // diesem Schritt nicht — nur einmal berechnet statt bei jedem
    // Kandidaten neu.
    let fixed_tokens = estimate_tokens(&context.system_context)
        + estimate_action_schemas_tokens(&context.available_actions);

    let max_cut = rounds.len() - min_preserved_rounds;
    let mut candidate = Vec::new();
    for cut_count in 1..=max_cut {
        candidate = vec![round_truncation_placeholder(cut_count)];
        for round in &rounds[cut_count..] {
            candidate.extend(round.iter().cloned());
        }
        let history_tokens: usize = candidate.iter().map(estimate_message_tokens).sum();
        if fixed_tokens + history_tokens <= budget_tokens {
            break;
        }
    }
    context.history = candidate;
}

fn round_truncation_placeholder(cut_rounds: usize) -> ChatMessage {
    ChatMessage {
        // `ActionResult` statt `User`/`Assistant`: dieser Eintrag stammt
        // weder vom Nutzer noch von der KI, sondern ist ein vom Backend
        // eingefügter Systemhinweis — dieselbe Kategorie wie
        // `CommandResult`/`ActionRejected` (beide ebenfalls `ActionResult`,
        // beide auf der Provider-Wire-Rolle "user" abgebildet, s.
        // `ai_providers::role_str`). Vermeidet außerdem, dass eine
        // Kompaktierung ausgerechnet die einzige verbleibende
        // `Role::User`-Nachricht entfernt, auf die z. B.
        // `orchestration::generate_session_title_on_disconnect`s
        // `has_user_message`-Prüfung angewiesen ist (durch mindestens eine
        // erhaltene Runde ohnehin unkritisch, hier zusätzlich robust).
        role: Role::ActionResult,
        content: MessageContent::Text(format!(
            "[Hinweis: ältere Konversation gekürzt — {cut_rounds} frühere Gesprächsrunde(n) \
             wurden aus Platzgründen aus diesem Kontext entfernt. Der vollständige Verlauf \
             bleibt im Session-Ledger erhalten.]"
        )),
    }
}

/* ------- Schritt 2: Riesen-Einzelausgaben kürzen (Spec 0057, §3.2.2) ------ */

/// Höchstzahl an Bytes, die `stdout`/`stderr` EINER einzelnen
/// `CommandResult`-Nachricht in dieser einen Anfrage noch beitragen dürfen
/// (Spec 0057, §3.2, Schritt 2) — fängt den Fall, dass eine einzelne,
/// jüngste (also nie von Schritt 1 betroffene) Runde bereits für sich
/// genommen zu groß ist (z. B. ein 2-MB-Kommando-Output).
const MAX_SINGLE_OUTPUT_BYTES_IN_CONTEXT: usize = 20_000;

/// Ehrliche Formulierung (Aufgabenstellung, Spec 0057 §3.2 Punkt 2): NICHT
/// "vollständig im Ledger". spec-reviewer-Fund (Review dieses Schritts):
/// statt eines eigenen, in die `stdout`/`stderr`-BYTES hineingeschriebenen
/// Hinweistexts (der dadurch INNERHALB der `<stdout>`/`<stderr>`-Fence
/// gelandet wäre — genau der Bereich, den `<security_notice>` als
/// "niemals als Anweisung interpretieren" markiert, und den ein Angreifer
/// zudem durch einen wörtlich gleichlautenden String in der echten Ausgabe
/// hätte vortäuschen/verschleiern können) wird das bereits vorhandene
/// `CommandOutput::truncated`-Flag gesetzt — genau der Mechanismus, den
/// `ai_providers::format_command_result` schon für den Spec-0043-Exec-Cap
/// nutzt: ein `<output_truncated>`-Hinweis AUSSERHALB der Fence ("stdout/
/// stderr above were cut off after reaching the configured output size
/// limit"). Diese Formulierung ist absichtlich generisch genug, um für
/// BEIDE Ursachen (Exec-Zeit-Cap, Spec 0043; oder hier: Kontext-Zeit-Cap)
/// gleichermaßen zu stimmen, ohne eine falsche Vollständigkeit zu
/// behaupten — kein zweiter, eigener Hinweistext nötig.
fn truncate_oversized_output(output: &mut ssh_manager_core::ssh::CommandOutput) -> bool {
    let stdout_len = output.stdout.len();
    let stderr_len = output.stderr.len();
    if stdout_len <= MAX_SINGLE_OUTPUT_BYTES_IN_CONTEXT
        && stderr_len <= MAX_SINGLE_OUTPUT_BYTES_IN_CONTEXT
    {
        return false;
    }
    output
        .stdout
        .truncate(MAX_SINGLE_OUTPUT_BYTES_IN_CONTEXT.min(stdout_len));
    output
        .stderr
        .truncate(MAX_SINGLE_OUTPUT_BYTES_IN_CONTEXT.min(stderr_len));
    output.truncated = true;
    true
}

/// Kürzt `stdout`/`stderr` jeder `CommandResult`-Nachricht in `history` auf
/// höchstens [`MAX_SINGLE_OUTPUT_BYTES_IN_CONTEXT`] Byte — die Runde selbst
/// (und damit die Nachricht) bleibt erhalten, nur ihr Inhalt schrumpft.
/// Läuft unbedingt über die GESAMTE übergebene Historie — anders als
/// [`compact_oversized_outputs_for_budget`] (die tatsächlich in
/// [`compact_for_send`] verwendete, budgetbewusste Variante, die aufhört
/// sobald der Request wieder passt) kürzt diese Variante ALLE
/// übergroßen Ausgaben unbedingt. Als eigenständiger, einfacher Baustein
/// erhalten (u. a. direkt getestet).
#[cfg(test)]
fn truncate_oversized_command_outputs(history: Vec<ChatMessage>) -> Vec<ChatMessage> {
    history
        .into_iter()
        .map(|mut message| {
            if let MessageContent::CommandResult { output, .. } = &mut message.content {
                truncate_oversized_output(output);
            }
            message
        })
        .collect()
}

/// Wie [`truncate_oversized_command_outputs`], aber budgetbewusst
/// (spec-reviewer-Fund, Review dieses Schritts: die Aufgabenstellung
/// verlangt "nur so weit wie nötig" — nicht jede übergroße Ausgabe
/// unbedingt kürzen, sondern in einer festen (chronologischen)
/// Reihenfolge aufhören, sobald der Request wieder unters Budget passt).
fn compact_oversized_outputs_for_budget(context: &mut SessionContext, budget_tokens: usize) {
    for index in 0..context.history.len() {
        if estimate_request_tokens(context) <= budget_tokens {
            return;
        }
        if let MessageContent::CommandResult { output, .. } = &mut context.history[index].content {
            truncate_oversized_output(output);
        }
    }
}

/* -------------------- System-Kontext / Notizen (Spec 0057, §4.1) ---------- */

/// Rohbestandteile eines Session-System-Prompts, getrennt gehalten statt
/// direkt zu einem einzigen String zusammengefügt (Spec 0057, §4.1: die
/// Kompaktierung muss Notiz-Abschnitte einzeln, nach Scope priorisiert,
/// kürzen können — ein Wiederaufsplitten aus dem bereits fertigen String
/// wäre gegen absichtlich in eine Notiz eingeschleusten Text wie
/// "## Remote-System" nicht robust, s. ADR 0048).
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct SystemContextParts {
    /// Einleitung + Werkzeug-Anweisungen + Freigegebene-Befehle-Block —
    /// stabil, wird von der Kompaktierung nie verändert.
    pub base: String,
    /// (Label, Notiztext)-Paare, UNGEFENCT, in aufsteigender Scope-
    /// Reichweite (allgemeinster Gruppen-Scope zuerst, server-spezifisch
    /// zuletzt — dieselbe Reihenfolge wie
    /// `ssh_manager_core::profiles::effective_notes_sections`). Spec 0057
    /// §4.1: bei Kürzung werden frühere (allgemeinere) Einträge zuerst
    /// gekürzt/entfernt, der letzte (server-spezifische) bleibt am
    /// längsten erhalten.
    pub note_sections: Vec<(String, String)>,
    pub remote_os_info: Option<String>,
}

impl SystemContextParts {
    pub fn has_notes(&self) -> bool {
        !self.note_sections.is_empty()
    }

    /// Baut den vollständigen System-Prompt aus den ungekürzten
    /// Bestandteilen — identisch zu dem, was `commands::
    /// build_session_system_context` vor dessen Aufteilung in
    /// [`SystemContextParts`] inline tat.
    pub fn assemble(&self) -> String {
        self.assemble_with_notes(&self.note_sections)
    }

    /// Wie [`Self::assemble`], aber mit explizit übergebenen (ggf. von der
    /// Kompaktierung gekürzten) `note_sections` statt `self.note_sections`
    /// — der Haken, über den [`compact_notes_for_budget`] eine verkürzte
    /// Fassung einsetzt, ohne `base`/`remote_os_info` anzufassen und ohne
    /// `self.note_sections` selbst zu verändern (die gespeicherte,
    /// vollständige Fassung bleibt in `SystemContextParts` unangetastet,
    /// Spec 0057 §4.1: "gespeicherte Notiz bleibt unangetastet").
    pub fn assemble_with_notes(&self, note_sections: &[(String, String)]) -> String {
        let mut context = self.base.clone();
        if !note_sections.is_empty() {
            context.push_str("\n\n## Notizen / Kontext\n");
            let fenced_sections: Vec<String> = note_sections
                .iter()
                .map(|(label, notes)| fence_untrusted(UntrustedKind::ServerNote, label, notes))
                .collect();
            context.push_str(&fenced_sections.join("\n\n"));
        }
        if let Some(os) = &self.remote_os_info {
            context.push_str(&format!("\n\n## Remote-System\n{os}"));
        }
        context
    }
}

/// Wird beim Kürzen einer Notiz-Sektion (Spec 0057, §4.1) an ihr Ende
/// gehängt — lautlos (keine Dialog-Unterbrechung, s. Spec 0057 §4.1,
/// letzter Satz), nur für DIESE eine Anfrage; die gespeicherte Notiz
/// bleibt unangetastet (s. [`SystemContextParts::assemble_with_notes`]-
/// Doc-Kommentar).
const NOTE_TRUNCATED_FOR_CONTEXT_NOTICE: &str =
    "\n[... für diese Anfrage gekürzt — die gespeicherte Notiz ist vollständig.]";

/// Untergrenze für die zuletzt verbleibende (server-spezifische, s. Spec
/// 0057 §4.1: "server-spezifische Notiz bleibt am längsten") Notiz-Sektion
/// — sie wird gekürzt, aber nie unter diese Byte-Zahl hinaus, damit auch
/// im Extremfall noch ein sinnvoller Rest ankommt statt eines
/// bedeutungslosen Fragments.
const MIN_LAST_NOTE_SECTION_BYTES: usize = 2_000;

/// Spec 0057, §3.2, Schritt 3 / §4.1: kürzt `context.system_context` durch
/// Entfernen/Kürzen von Notiz-Sektionen, bis `context` wieder unter
/// `budget_tokens` passt — oder bis nur noch die server-spezifische
/// Sektion (auf ihre Untergrenze gekürzt) übrig ist. Priorisierung nach
/// Scope (ADR 0003/0004, ausgeführt in Spec 0057 §4.1): `parts.
/// note_sections` ist in aufsteigender Scope-Reichweite sortiert
/// (allgemeinster Gruppen-Scope zuerst, server-spezifisch zuletzt) —
/// frühere (Index 0..) Einträge werden zuerst komplett entfernt, der
/// letzte nur als letztes Mittel gekürzt, nie entfernt. `parts` selbst
/// (und damit die darin gehaltene VOLLSTÄNDIGE Notiz) bleibt unverändert —
/// nur die lokale Kopie `sections` wird gekürzt.
fn compact_notes_for_budget(
    context: &mut SessionContext,
    parts: &SystemContextParts,
    budget_tokens: usize,
) {
    let mut sections = parts.note_sections.clone();
    while sections.len() > 1 && estimate_request_tokens(context) > budget_tokens {
        sections.remove(0);
        context.system_context = parts.assemble_with_notes(&sections);
    }
    if sections.len() == 1 && estimate_request_tokens(context) > budget_tokens {
        let (label, text) = sections[0].clone();
        // Grobe Ziel-Byte-Zahl aus dem Token-Überschuss zurückgerechnet
        // (dieselbe ~4-Byte-pro-Token-Heuristik wie `estimate_tokens`) —
        // muss nicht exakt sein, die `while`-Schleife oben deckt bereits
        // den Regelfall ab, hier geht es nur noch um den Rest.
        let overshoot_tokens = estimate_request_tokens(context).saturating_sub(budget_tokens);
        let target_bytes = text
            .len()
            .saturating_sub(overshoot_tokens * BYTES_PER_TOKEN_ESTIMATE)
            .max(MIN_LAST_NOTE_SECTION_BYTES)
            .min(text.len());
        let mut truncated_text = truncate_to_char_boundary(&text, target_bytes).to_string();
        truncated_text.push_str(NOTE_TRUNCATED_FOR_CONTEXT_NOTICE);
        sections[0] = (label, truncated_text);
        context.system_context = parts.assemble_with_notes(&sections);
    }
}

/// Schneidet `text` auf höchstens `max_bytes` Byte, rückt aber ggf. auf die
/// nächste gültige UTF-8-Zeichengrenze zurück (`&str`, anders als
/// [`truncate_oversized_output`]s rohe `Vec<u8>`, MUSS an einer
/// Zeichengrenze enden).
fn truncate_to_char_boundary(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/* --------------------- Die Kürzungs-Reihenfolge (Spec 0057, §3.2) --------- */

/// Zentrale Kompaktierungs-Funktion (Spec 0057, §3) — wird unmittelbar vor
/// jedem `AiProvider::send()`-Aufruf auf die geklonte Kontext-Kopie
/// angewendet (s. `orchestration::run_one_round`/`generate_session_title_
/// on_disconnect`/`suggest_note_update_on_disconnect`), NIE auf
/// `session.context` selbst — die im Frontend angezeigte, vollständige
/// Historie und die gespeicherte Notiz bleiben in jedem Fall unverändert
/// (Spec 0057, §3.3/§4.1/§6). Das Ledger (Etappe 1) bekommt diese Kürzung
/// nie zu Gesicht — es protokolliert an ganz anderer Stelle
/// (`orchestration::write_ledger_entry`), unabhängig davon, ob hier je
/// kompaktiert wird oder nicht (Spec 0057, §3.3: "Kompaktierung betrifft
/// nur den an die KI gesendeten Kontext, nie den Ledger").
///
/// No-op (gibt `context` unverändert zurück), wenn die geschätzte
/// Anfragegröße bereits unter [`COMPACTION_TRIGGER_RATIO`] des
/// Kontextfensters liegt — der Regelfall für die meisten Anfragen.
/// Andernfalls die Kürzungs-Reihenfolge aus Spec 0057 §3.2, jeweils nur so
/// weit wie nötig:
/// 1. Alte Runden → Platzhalter (letzte [`MIN_PRESERVED_ROUNDS`] immer voll).
/// 2. Riesen-Einzelausgaben in den erhaltenen Runden kürzen.
/// 3. Notiz verkürzt senden (nach Scope priorisiert, lautlos).
pub(crate) fn compact_for_send(
    mut context: SessionContext,
    system_context_parts: &SystemContextParts,
    model_context_window_tokens: usize,
) -> SessionContext {
    let budget_tokens = (model_context_window_tokens as f64 * COMPACTION_TRIGGER_RATIO) as usize;

    if estimate_request_tokens(&context) <= budget_tokens {
        return context;
    }
    tracing::info!(
        budget_tokens,
        model_context_window_tokens,
        "estimated request size exceeds the compaction trigger — compacting context for this send"
    );

    compact_rounds_for_budget(&mut context, MIN_PRESERVED_ROUNDS, budget_tokens);
    if estimate_request_tokens(&context) <= budget_tokens {
        return context;
    }

    compact_oversized_outputs_for_budget(&mut context, budget_tokens);
    if estimate_request_tokens(&context) <= budget_tokens {
        return context;
    }

    compact_notes_for_budget(&mut context, system_context_parts, budget_tokens);

    // spec-reviewer-Fund (Review dieses Schritts): selbst die volle
    // Kürzungs-Leiter hat Untergrenzen (mindestens `MIN_PRESERVED_ROUNDS`
    // Runden, bis zu `MAX_SINGLE_OUTPUT_BYTES_IN_CONTEXT` pro Ausgabe,
    // mindestens `MIN_LAST_NOTE_SECTION_BYTES` der letzten Notiz-Sektion)
    // — bei einem sehr kleinen Kontextfenster (z. B.
    // `DEFAULT_CONTEXT_WINDOW_TOKENS` für ein unbekanntes Modell) können
    // diese Untergrenzen zusammen das Budget immer noch überschreiten.
    // Sichtbar im Log statt lautlos einen weiterhin übergroßen Request
    // abzuschicken — Spec 0057 §2.2/§6 fordert für den (in Etappe 3
    // kommenden) Summary-Fallback explizit "Fehler containen, nicht
    // stillschweigend verschlucken"; dieselbe Haltung gilt hier.
    let final_tokens = estimate_request_tokens(&context);
    if final_tokens > budget_tokens {
        tracing::warn!(
            final_tokens,
            budget_tokens,
            model_context_window_tokens,
            "context compaction reached its floor (min preserved rounds / per-output cap / \
             min note size) but the request is still over budget — sending anyway"
        );
    }
    context
}

#[cfg(test)]
mod tests {
    use super::*;
    use ssh_manager_core::ssh::CommandOutput;

    fn text_message(role: Role, text: &str) -> ChatMessage {
        ChatMessage {
            role,
            content: MessageContent::Text(text.to_string()),
        }
    }

    fn user_message(text: &str) -> ChatMessage {
        text_message(Role::User, text)
    }

    fn command_result(command: &str, stdout: &str) -> ChatMessage {
        ChatMessage {
            role: Role::ActionResult,
            content: MessageContent::CommandResult {
                command: command.to_string(),
                output: CommandOutput {
                    stdout: stdout.as_bytes().to_vec(),
                    stderr: Vec::new(),
                    exit_code: Some(0),
                    truncated: false,
                },
                cancelled: false,
            },
        }
    }

    fn context_with(system_context: String, history: Vec<ChatMessage>) -> SessionContext {
        SessionContext {
            system_context,
            history,
            available_actions: Vec::new(),
        }
    }

    // --- Token-Schätzung, inkl. Fencing-Expansion (Spec 0057, §7) ---------

    #[test]
    fn test_estimate_tokens_uses_four_bytes_per_token_heuristic() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("ab"), 1);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
    }

    /// Spec 0057, §3.1: "Post-Fencing gerechnet" — ein Kommando-Output
    /// voller `<`/`>`/`&` muss DEUTLICH mehr Token schätzen als eine
    /// naive, rohe Zeichenzählung ergäbe (die alte, abgelöste
    /// `chat_context_truncation::approximate_char_len` hätte das
    /// systematisch unterschätzt, s. Moduldoc).
    #[test]
    fn test_estimate_message_tokens_accounts_for_fencing_expansion() {
        let raw_output = "<script>".repeat(200); // viele `<`/`>` = starke Expansion
        let message = command_result("cat evil.html", &raw_output);
        let estimated = estimate_message_tokens(&message);

        let naive_raw_char_estimate = estimate_tokens(&raw_output);
        assert!(
            estimated > naive_raw_char_estimate,
            "Post-Fencing-Schätzung ({estimated}) muss über der naiven \
             Rohzeichen-Schätzung ({naive_raw_char_estimate}) liegen"
        );

        // Exakte Gegenprobe: die Schätzung muss dem tatsächlichen,
        // gefenceten Text entsprechen (plus fester Overhead), nicht nur
        // "irgendwie größer" sein.
        let actually_fenced =
            fence_untrusted(UntrustedKind::CommandStdout, "cat evil.html", &raw_output);
        assert!(estimated >= estimate_tokens(&actually_fenced));
    }

    #[test]
    fn test_estimate_request_tokens_sums_system_context_history_and_actions() {
        let context = context_with("x".repeat(400), vec![user_message(&"y".repeat(400))]);
        let estimated = estimate_request_tokens(&context);
        assert_eq!(estimated, 100 + 100); // 400/4 + 400/4, keine Actions
    }

    // --- Auslöser bei ~70-80 % (Spec 0057, §7) -----------------------------

    #[test]
    fn test_compact_for_send_is_noop_below_trigger_ratio() {
        // Kontextfenster 1000 Token, Budget = 750 — 600 Token bleiben
        // unangetastet.
        let context = context_with(String::new(), vec![user_message(&"a".repeat(2400))]); // ~600 Token
        let parts = SystemContextParts::default();
        let result = compact_for_send(context.clone(), &parts, 1_000);
        assert_eq!(
            result, context,
            "unterhalb des Auslösers darf nichts verändert werden"
        );
    }

    #[test]
    fn test_compact_for_send_triggers_above_trigger_ratio() {
        // 4 kurze Runden + eine Riesen-Ausgabe (100.000 Byte, weit über
        // `MAX_SINGLE_OUTPUT_BYTES_IN_CONTEXT`) in der jüngsten Runde —
        // weit über 75 % von 10.000 Token (Budget 7.500) -> muss auslösen
        // UND Schritt 2 (nicht nur Schritt 1) tatsächlich durchlaufen, um
        // wieder unters Budget zu kommen.
        let history = vec![
            user_message("Runde 1"),
            user_message("Runde 2"),
            user_message("Runde 3"),
            user_message("Runde 4"),
            command_result("cat big.log", &"a".repeat(100_000)),
        ];
        let context = context_with(String::new(), history.clone());
        let parts = SystemContextParts::default();
        let result = compact_for_send(context.clone(), &parts, 10_000);
        assert_ne!(
            result, context,
            "oberhalb des Auslösers (~70-80 %) muss kompaktiert werden"
        );
        assert!(estimate_request_tokens(&result) <= 7_500);
    }

    // --- Schritt 1: letzte N Runden bleiben immer voll erhalten -----------

    #[test]
    fn test_split_into_rounds_groups_by_user_message_boundaries() {
        let history = vec![
            user_message("Frage 1"),
            command_result("ls", "ok"),
            user_message("Frage 2"),
        ];
        let rounds = split_into_rounds(history);
        assert_eq!(rounds.len(), 2);
        assert_eq!(rounds[0].len(), 2);
        assert_eq!(rounds[1].len(), 1);
    }

    #[test]
    fn test_truncate_rounds_with_placeholder_keeps_last_n_rounds_fully_intact() {
        let history = vec![
            user_message("Runde 1"),
            command_result("cmd1", "out1"),
            user_message("Runde 2"),
            command_result("cmd2", "out2"),
            user_message("Runde 3"),
            command_result("cmd3", "out3"),
            user_message("Runde 4"),
            command_result("cmd4", "out4"),
        ];
        let result = truncate_rounds_with_placeholder(history.clone(), 3);

        // Platzhalter + die letzten 3 Runden (je 2 Nachrichten) = 7.
        assert_eq!(result.len(), 1 + 6);
        // Die letzten 3 Runden müssen BYTE-IDENTISCH erhalten sein.
        assert_eq!(&result[1..], &history[2..]);
    }

    #[test]
    fn test_truncate_rounds_with_placeholder_is_noop_when_within_limit() {
        let history = vec![user_message("a"), user_message("b")];
        let result = truncate_rounds_with_placeholder(history.clone(), 3);
        assert_eq!(result, history);
    }

    /// spec-reviewer-Fund (Review dieses Schritts): Schritt 1 darf nicht
    /// unbedingt auf `MIN_PRESERVED_ROUNDS` kürzen, sondern nur so viele
    /// alte Runden entfernen, wie tatsächlich nötig sind, um wieder unters
    /// Budget zu kommen ("nur so weit wie nötig", Aufgabenstellung) —
    /// reicht das Entfernen EINER alten Runde bereits, müssen die übrigen
    /// (auch über `MIN_PRESERVED_ROUNDS` hinaus) unangetastet bleiben.
    #[test]
    fn test_compact_rounds_for_budget_stops_as_soon_as_it_fits() {
        let history = vec![
            user_message("alte Runde 1"),
            user_message("alte Runde 2"),
            user_message(&"a".repeat(20_000)), // treibt die Gesamtgröße hoch
            user_message("Runde 4"),
            user_message("Runde 5"),
        ];
        // Budget genau so bemessen, dass das Entfernen NUR der ältesten
        // Runde ("alte Runde 1") bereits reicht.
        let mut budget_probe = context_with(String::new(), history.clone());
        compact_rounds_for_budget(&mut budget_probe, 4, usize::MAX); // 1 Runde entfernt (5 > 4)
        let budget = estimate_request_tokens(&budget_probe);

        let mut context = context_with(String::new(), history);
        compact_rounds_for_budget(&mut context, 1, budget);

        assert!(
            context.history.iter().any(
                |m| matches!(&m.content, MessageContent::Text(t) if t.contains("alte Runde 2"))
            ),
            "Runde 2 hätte nicht entfernt werden dürfen, das Budget passte schon nach Runde 1: \
             {:?}",
            context.history
        );
    }

    // --- Schritt 2: Riesen-Einzelausgabe gekürzt, Runde nicht verworfen ---

    #[test]
    fn test_truncate_oversized_command_outputs_shrinks_but_keeps_the_message() {
        let huge_output = "x".repeat(500_000);
        let history = vec![
            user_message("Frage"),
            command_result("cat big.log", &huge_output),
        ];
        let result = truncate_oversized_command_outputs(history);

        assert_eq!(
            result.len(),
            2,
            "die Runde/Nachricht darf nicht verworfen werden"
        );
        let MessageContent::CommandResult { output, .. } = &result[1].content else {
            panic!("erwartete CommandResult");
        };
        assert!(
            output.stdout.len() < 500_000,
            "die riesige Ausgabe muss gekürzt worden sein"
        );
        // spec-reviewer-Fund (Review dieses Schritts): kein eigener,
        // in die Bytes hineingeschriebener Hinweistext mehr (der wäre
        // INNERHALB der `<stdout>`-Fence gelandet und durch echten
        // Ausgabeinhalt vortäuschbar) — stattdessen dasselbe
        // `truncated`-Flag wie beim Spec-0043-Exec-Cap, das
        // `ai_providers::format_command_result` bereits als
        // `<output_truncated>`-Hinweis AUSSERHALB der Fence rendert.
        assert!(
            output.truncated,
            "muss das bestehende truncated-Flag setzen, keinen eigenen In-Fence-Hinweistext"
        );
    }

    #[test]
    fn test_truncate_oversized_command_outputs_leaves_small_output_untouched() {
        let history = vec![command_result("ls", "total 0")];
        let result = truncate_oversized_command_outputs(history.clone());
        assert_eq!(result, history);
    }

    /// spec-reviewer-Fund (Review dieses Schritts): die budgetbewusste
    /// Variante darf NICHT jede übergroße Ausgabe unbedingt kürzen —
    /// reicht das Kürzen der ersten (ältesten) schon, um wieder unters
    /// Budget zu kommen, muss die zweite unangetastet bleiben ("nur so
    /// weit wie nötig", Aufgabenstellung).
    #[test]
    fn test_compact_oversized_outputs_for_budget_stops_as_soon_as_it_fits() {
        let history = vec![
            command_result("cat a.log", &"a".repeat(30_000)),
            command_result("cat b.log", &"b".repeat(30_000)),
        ];
        // Budget dynamisch bestimmt: genau die Größe, die entsteht, wenn
        // NUR die erste (älteste) Ausgabe gekürzt ist — die zweite bleibt
        // dann bewusst über dem Cap, muss aber trotzdem unangetastet
        // bleiben, weil das Budget an dieser Stelle schon erreicht ist.
        let mut budget_probe = context_with(String::new(), history.clone());
        let MessageContent::CommandResult { output, .. } = &mut budget_probe.history[0].content
        else {
            panic!("erwartete CommandResult");
        };
        truncate_oversized_output(output);
        let budget = estimate_request_tokens(&budget_probe);

        let mut context = context_with(String::new(), history);
        compact_oversized_outputs_for_budget(&mut context, budget);

        let MessageContent::CommandResult { output: first, .. } = &context.history[0].content
        else {
            panic!("erwartete CommandResult");
        };
        let MessageContent::CommandResult { output: second, .. } = &context.history[1].content
        else {
            panic!("erwartete CommandResult");
        };
        assert!(
            first.truncated,
            "die erste (älteste) Ausgabe muss gekürzt werden"
        );
        assert!(
            !second.truncated,
            "die zweite Ausgabe darf nicht angefasst werden, sobald das Budget schon passt"
        );
    }

    // --- Schritt 3: Notiz verkürzt, Scope-Priorisierung -------------------

    #[test]
    fn test_compact_notes_for_budget_drops_general_scope_sections_first() {
        let parts = SystemContextParts {
            base: "Basis".to_string(),
            note_sections: vec![
                ("Gruppe \"Global\"".to_string(), "g".repeat(3000)),
                ("Server \"db1\"".to_string(), "s".repeat(200)),
            ],
            remote_os_info: None,
        };
        let mut context = context_with(parts.assemble(), Vec::new());
        // Budget knapp über dem, was die Server-Notiz allein braucht.
        let budget =
            estimate_tokens(&parts.assemble_with_notes(&[parts.note_sections[1].clone()])) + 5;

        compact_notes_for_budget(&mut context, &parts, budget);

        assert!(
            !context.system_context.contains("Gruppe"),
            "die allgemeinere Gruppen-Notiz muss zuerst entfernt worden sein: {}",
            context.system_context
        );
        assert!(
            context.system_context.contains("db1"),
            "die server-spezifische Notiz muss (mindestens teilweise) erhalten bleiben: {}",
            context.system_context
        );
        // Die ORIGINAL-Sektionen in `parts` selbst dürfen nie verändert
        // werden (Spec 0057 §4.1: gespeicherte Notiz bleibt unangetastet).
        assert_eq!(parts.note_sections[0].1.len(), 3000);
    }

    #[test]
    fn test_compact_notes_for_budget_shrinks_last_section_instead_of_dropping_it() {
        let parts = SystemContextParts {
            base: "Basis".to_string(),
            note_sections: vec![("Server \"db1\"".to_string(), "s".repeat(50_000))],
            remote_os_info: None,
        };
        let mut context = context_with(parts.assemble(), Vec::new());
        let budget = 100; // sehr klein — zwingt zur Kürzung der letzten Sektion.

        compact_notes_for_budget(&mut context, &parts, budget);

        assert!(
            context.system_context.contains("db1"),
            "die letzte Sektion wird gekürzt, nie ganz entfernt"
        );
        assert!(
            context.system_context.len() < parts.assemble().len(),
            "der gesendete Kontext muss kleiner sein als das Original"
        );
        assert!(
            context.system_context.contains("für diese Anfrage gekürzt"),
            "muss den Kürzungs-Hinweis tragen: {}",
            context.system_context
        );
        // Untergrenze eingehalten (Notiztext + Label + Fence-Tags + Hinweis
        // bleiben spürbar über 0, s. `MIN_LAST_NOTE_SECTION_BYTES`).
        assert!(context.system_context.len() >= MIN_LAST_NOTE_SECTION_BYTES);
    }

    // --- Modell-Kontextfenster ---------------------------------------------

    #[test]
    fn test_model_context_window_tokens_known_and_unknown_models() {
        assert_eq!(
            model_context_window_tokens(ProviderType::Anthropic, "claude-sonnet-4-5"),
            200_000
        );
        assert_eq!(
            model_context_window_tokens(ProviderType::OpenAi, "gpt-3.5-turbo"),
            16_000
        );
        assert_eq!(
            model_context_window_tokens(ProviderType::OpenAi, "gpt-4o"),
            128_000
        );
        assert_eq!(
            model_context_window_tokens(ProviderType::Ollama, "some-tiny-local-model"),
            DEFAULT_CONTEXT_WINDOW_TOKENS
        );
        assert_eq!(
            model_context_window_tokens(ProviderType::GenericOpenAiCompatible, "unknown"),
            DEFAULT_CONTEXT_WINDOW_TOKENS
        );
    }
}
