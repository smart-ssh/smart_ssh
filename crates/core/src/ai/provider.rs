use std::pin::Pin;

use futures::Stream;

use super::types::{AiEvent, SessionContext};

/// Einheitliche Abstraktion über eine frei wählbare KI (OpenAI, Anthropic,
/// lokale Modelle via Ollama, generischer OpenAI-kompatibler Endpoint),
/// Spec 0006 Abschnitt 3.
///
/// **Wichtige Abgrenzung** (Spec 0006, Abschnitt 1): dieser Trait schlägt
/// Aktionen nur vor. Er führt nichts aus und umgeht nie die Filter-Engine
/// (Spec 0002) — jede `SuggestCommand`-Aktion durchläuft unverändert deren
/// Präzedenz-Kette, unabhängig davon, welcher Provider sie erzeugt hat.
///
/// Bewusst **ohne** `#[async_trait]`, obwohl die Spec-Skizze das Attribut
/// zeigt: `send()` selbst ist keine `async fn` (die einzige Methode, die
/// `async_trait` transformieren würde), sondern eine synchrone Methode, die
/// einen bereits dyn-kompatiblen `Pin<Box<dyn Stream<...> + Send>>`
/// zurückgibt — die eigentliche Netzwerk-I/O passiert beim *Pollen* dieses
/// Streams, nicht beim Aufruf von `send()` selbst. `async_trait` hätte hier
/// nichts zu transformieren.
pub trait AiProvider: Send + Sync {
    fn send(&self, context: SessionContext) -> Pin<Box<dyn Stream<Item = AiEvent> + Send>>;
}
