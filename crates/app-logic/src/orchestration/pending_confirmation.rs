//! Spec 0088, A1/A2: Das Warten auf eine Bestätigung als Wert, dessen `Drop`
//! aufräumt — statt eines Setzens und Zurücksetzens von Hand rund um den
//! `await`.
//!
//! Vorher standen die beiden Hälften des Aufräumens als getrennte
//! Anweisungen vor und nach `timeout(…, rx).await` in
//! `action_exec::handle_action_proposed`. Damit räumte jeder Weg auf, den
//! der Code selbst nimmt — aber kein Weg, den er *nicht* nimmt: Wird der
//! wartende Future fallen gelassen (Abbruch der Task, `select!`, der den
//! anderen Zweig gewinnen lässt), bleiben `Session::pending_action` auf
//! `Some` und der Eintrag in der [`ConfirmationRegistry`] stehen. Sichtbar
//! wird das als Tab-Indikator „wartet auf Bestätigung", zu dem es keine
//! Aktion mehr gibt, die man bestätigen könnte.
//!
//! Ein `Drop` läuft auf **jedem** dieser Wege, auch beim Abwickeln eines
//! Panics. Genau deshalb darf er selbst nie panicken (A2.2) — ein Panic im
//! `Drop` während des Abwickelns bricht den Prozess ab.

use tokio::sync::oneshot;

use crate::confirmation::{ConfirmationRegistry, RegistrationGeneration};
use crate::dto::ActionUserDecision;
use crate::poison::lock_tolerating_poison;
use crate::session::Session;
use crate::state::ActionId;

/// Eine registrierte, noch nicht aufgelöste Bestätigung für `action_id`.
///
/// Lebt von der Registrierung (vor den `await`s für Vorschau und
/// Zweitmeinung, damit ein Klick währenddessen nicht verloren geht) bis zu
/// dem Punkt, an dem die Entscheidung feststeht. Solange dieser Wert lebt,
/// gilt: Registry-Eintrag vorhanden, und ab
/// [`PendingConfirmation::wait_for_decision`] zusätzlich der Tab-Indikator
/// gesetzt. Sobald er fällt, ist beides abgeräumt.
///
/// Der Wert wird bewusst an einen benannten Binding gebunden und erst nach
/// dem Warten verworfen — ein `let _ = …` würde ihn sofort fallen lassen und
/// damit abräumen, bevor der Nutzer überhaupt antworten kann (Spec 0088,
/// T14 sichert das ab).
pub(crate) struct PendingConfirmation<'a> {
    cleanup: ConfirmationCleanup<'a>,
    receiver: oneshot::Receiver<ActionUserDecision>,
}

/// Issue #108: In dieser (MCP-)Sitzung wartet bereits eine Bestätigung.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AnotherConfirmationPending;

/// Fehlertext an den MCP-Client, wenn ein weiterer Vorschlag abgewiesen wird.
pub(crate) const ANOTHER_CONFIRMATION_PENDING_MESSAGE: &str = "Another action is awaiting confirmation in this session. Decide or reject the pending action in the app first, then propose again.";

/// Ausgang eines Wartens: äußeres `Err` = Zeitgrenze erreicht, inneres
/// `Err` = der Sender wurde gedroppt.
pub(crate) type ConfirmationWaitOutcome =
    Result<Result<ActionUserDecision, oneshot::error::RecvError>, tokio::time::error::Elapsed>;

impl<'a> PendingConfirmation<'a> {
    /// Registriert `action_id` als wartend. Ab hier räumt [`Drop`] auf.
    /// Der Tab-Indikator wird erst in [`Self::wait_for_decision`] gesetzt.
    ///
    /// `exclusive` (MCP-Sitzungen, Spec 0104 §5): Wartet in der Sitzung
    /// bereits eine Bestätigung, wird nichts registriert und
    /// [`AnotherConfirmationPending`] zurückgegeben. Prüfung und Belegung
    /// geschehen in einem atomaren Schritt (von der Registrierung bis zum Fallen), damit parallele Vorschläge nicht
    /// beide durchkommen. Nicht exklusiv ändert sich nichts gegenüber vorher.
    pub(crate) fn register(
        session: &'a Session,
        registry: &'a ConfirmationRegistry<ActionId, ActionUserDecision>,
        action_id: ActionId,
        exclusive: bool,
    ) -> Result<Self, AnotherConfirmationPending> {
        if exclusive
            && session
                .mcp_confirmation_claim
                .compare_exchange(
                    false,
                    true,
                    std::sync::atomic::Ordering::SeqCst,
                    std::sync::atomic::Ordering::SeqCst,
                )
                .is_err()
        {
            return Err(AnotherConfirmationPending);
        }
        let RegisteredConfirmation { registry, receiver } =
            RegisteredConfirmation::register(registry, action_id);
        Ok(Self {
            cleanup: ConfirmationCleanup {
                session,
                registry,
                claimed: exclusive,
            },
            receiver,
        })
    }

    /// Setzt den Tab-Indikator (Spec 0017, Abschnitt 5:
    /// `SessionSummaryDto.has_pending_action`) und wartet bis zur
    /// Entscheidung oder `timeout`.
    ///
    /// Beides in einer Funktion, damit der Indikator nicht gesetzt werden
    /// kann, ohne dass danach gewartet wird — und damit das Zurücksetzen
    /// nicht mehr an einer zweiten Anweisung hängt, die ein späterer
    /// Fehlerpfad überspringen könnte.
    ///
    /// **Verbraucht `self` und gibt das Aufräumen zurück**
    /// (spec-reviewer-Fund, Runde 1): Ein `oneshot::Receiver` darf nach
    /// seiner Auflösung nicht erneut gepollt werden — tokio panickt dann.
    /// Nähme diese Funktion `&mut self`, wäre ein zweites Warten möglich und
    /// die Zusage „genau einmal" stünde wieder nur im Text. So kann der
    /// Aufrufer sie per Konstruktion nur einmal aufrufen; das Aufräumen lebt
    /// im zurückgegebenen [`ConfirmationCleanup`] weiter, bis er es fallen
    /// lässt. Wird der Future dieses Aufrufs währenddessen fallen gelassen,
    /// fällt das `ConfirmationCleanup` mit ihm und räumt ab (A1.1/A1.2).
    pub(crate) async fn wait_for_decision(
        self,
        timeout: std::time::Duration,
    ) -> (ConfirmationWaitOutcome, ConfirmationCleanup<'a>) {
        let Self { cleanup, receiver } = self;
        // A2.1: auch bei vergifteter Sperre setzen statt panicken.
        *lock_tolerating_poison(&cleanup.session.pending_action) = Some(cleanup.registry.action_id);
        let outcome = tokio::time::timeout(timeout, receiver).await;
        (outcome, cleanup)
    }
}

/// Die aufräumende Hälfte einer [`PendingConfirmation`] — lebt weiter,
/// nachdem der Empfänger verbraucht ist, und räumt beim Fallen Tab-Indikator
/// und Registry-Eintrag ab.
///
/// Issue #107: Das Abräumen des Registry-Eintrags steckt in
/// [`RegistryCleanup`], damit app-weite Bestätigungen ohne Tab
/// ([`RegisteredConfirmation`]) dieselbe und nur diese eine Implementierung
/// nutzen. Hier kommt nur der Tab-Indikator dazu.
pub(crate) struct ConfirmationCleanup<'a> {
    session: &'a Session,
    /// Fällt (und räumt den Registry-Eintrag ab) erst, nachdem
    /// [`Drop::drop`] unten den Indikator zurückgesetzt hat — Felder werden
    /// nach dem `Drop` des umschließenden Werts verworfen. Die Reihenfolge
    /// ist damit dieselbe wie vor der Aufteilung.
    registry: RegistryCleanup<'a>,
    /// Issue #108: gibt beim Fallen den exklusiven Platz der MCP-Sitzung frei.
    claimed: bool,
}

impl Drop for ConfirmationCleanup<'_> {
    fn drop(&mut self) {
        if self.claimed {
            self.session
                .mcp_confirmation_claim
                .store(false, std::sync::atomic::Ordering::SeqCst);
        }
        {
            // Spec 0088, §5: nur den EIGENEN Indikator löschen. MCP und Chat
            // teilen sich eine `Session`; steht dort inzwischen die
            // `action_id` einer anderen wartenden Aktion, bleibt deren
            // Indikator stehen.
            //
            // A2.2: `lock_tolerating_poison` statt `lock().unwrap()` — ein
            // Panic hier, während bereits ein anderer Panic abgewickelt
            // wird, bräche den Prozess ab.
            let mut pending = lock_tolerating_poison(&self.session.pending_action);
            if *pending == Some(self.registry.action_id) {
                *pending = None;
            }
        }
        // Der Registry-Eintrag wird danach vom Feld `registry`
        // ([`RegistryCleanup`]) abgeräumt.
    }
}

/// Issue #107: Eine registrierte, noch nicht aufgelöste Bestätigung **ohne**
/// Tab-Indikator — für app-weite Benachrichtigungen, die an keinen Tab
/// gebunden sind (Notiz-Vorschlag beim Verbindungsende, Notiz-Kürzung).
/// Setzt `Session::pending_action` nie; hält dafür gar keine `Session`.
///
/// Sonst gilt dasselbe wie für [`PendingConfirmation`]: an einen benannten
/// Binding binden, und ab der Registrierung räumt `Drop` den
/// Registry-Eintrag auf jedem Weg ab — Entscheidung, gedroppter Sender,
/// Zeitgrenze, fallen gelassener Future, Panic.
pub(crate) struct RegisteredConfirmation<'a> {
    registry: RegistryCleanup<'a>,
    receiver: oneshot::Receiver<ActionUserDecision>,
}

impl<'a> RegisteredConfirmation<'a> {
    /// Registriert `action_id` als wartend. Ab hier räumt [`Drop`] auf.
    pub(crate) fn register(
        registry: &'a ConfirmationRegistry<ActionId, ActionUserDecision>,
        action_id: ActionId,
    ) -> Self {
        let (generation, receiver) = registry.register_tracked(action_id);
        Self {
            registry: RegistryCleanup {
                registry,
                action_id,
                generation,
            },
            receiver,
        }
    }

    /// Wartet bis zur Entscheidung oder `timeout`. Verbraucht `self` aus
    /// demselben Grund wie [`PendingConfirmation::wait_for_decision`]: Der
    /// Empfänger darf nach seiner Auflösung nicht erneut gepollt werden. Das
    /// zurückgegebene [`RegistryCleanup`] räumt beim Fallen ab.
    pub(crate) async fn wait_for_decision(
        self,
        timeout: std::time::Duration,
    ) -> (ConfirmationWaitOutcome, RegistryCleanup<'a>) {
        let Self { registry, receiver } = self;
        let outcome = tokio::time::timeout(timeout, receiver).await;
        (outcome, registry)
    }
}

/// Räumt beim Fallen den Registry-Eintrag DIESER Registrierung ab. Die
/// einzige Stelle, an der ein wartendes Bestätigen seinen Registry-Eintrag
/// entfernt — genutzt von [`ConfirmationCleanup`] und
/// [`RegisteredConfirmation`].
pub(crate) struct RegistryCleanup<'a> {
    registry: &'a ConfirmationRegistry<ActionId, ActionUserDecision>,
    action_id: ActionId,
    /// Spec 0068, Teil 5b: Nur der Eintrag DIESER Registrierung wird
    /// abgeräumt. `action_id`s werden heute nicht wiederverwendet, aber die
    /// Prüfung kostet nichts und macht den Guard unabhängig von dieser
    /// Zusage.
    generation: RegistrationGeneration,
}

impl Drop for RegistryCleanup<'_> {
    fn drop(&mut self) {
        // Nach `resolve` ist der Eintrag ohnehin weg; dann ist das hier ein
        // No-Op. Nach Timeout oder Abbruch räumt es ihn ab, damit ein
        // späteres `resolve` für diese `action_id` fehlschlägt statt
        // irgendetwas auszulösen (A1.2/A1.3). Die Registry sperrt intern
        // vergiftungstolerant (A2.2) — kein Panic im `Drop`.
        self.registry
            .cancel_if_current(&self.action_id, self.generation);
    }
}
