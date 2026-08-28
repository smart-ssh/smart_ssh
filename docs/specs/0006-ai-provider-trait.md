# Spec: KI-Provider-Trait

Status: Entwurf
Modul: Trait-Definitionen in `crates/core/src/ai/`, konkrete Implementierungen
in neuer Crate `crates/ai-providers`
Abhängigkeiten: `ssh-manager-core` (nutzt `AiAction`/`NoteTarget` aus Spec
0003, `CommandOutput` aus Spec 0005; Vorschläge laufen anschließend durch die
Filter-Engine aus Spec 0002 — dieses Modul selbst führt nichts aus)

## 1. Ziel

Einheitliche Abstraktion, über die die App mit einer frei wählbaren KI
(OpenAI, Anthropic, lokale Modelle via Ollama, generischer
OpenAI-kompatibler Endpoint) chattet, Kommandovorschläge und
Notiz-Änderungsvorschläge (`AiAction` aus Spec 0003) strukturiert entgegennimmt,
und Kommando-Ergebnisse für den nächsten Gesprächsschritt zurückspielt.

**Wichtige Abgrenzung**: Dieses Modul schlägt Aktionen nur vor. Es führt
nichts aus und umgeht nie die Filter-Engine (Spec 0002) — jede
`SuggestCommand`-Aktion durchläuft unverändert die dortige Präzedenz-Kette,
unabhängig davon, welcher Provider sie erzeugt hat.

## 2. Architektur-Entscheidung: Trait in `core`, Implementierungen separat

Gleiches Muster wie bei Persistenz (Spec 0004) und SSH (Spec 0005): `core`
definiert nur `AiProvider` und die zugehörigen Datentypen, konkrete
Provider-Implementierungen leben in `crates/ai-providers`. Innerhalb dieser
Crate werden OpenAI, ein generischer OpenAI-kompatibler Endpoint und Ollama
(das im OpenAI-kompatiblen Modus läuft) über **eine gemeinsame Implementierung**
abgedeckt, da sie dasselbe Request/Response-Schema teilen — nur Anthropic
braucht wegen des abweichenden Tool-Calling-Formats eine eigene
Implementierung.

## 3. Kernabstraktionen

```rust
#[async_trait]
pub trait AiProvider: Send + Sync {
    fn send(&self, context: SessionContext) -> Pin<Box<dyn Stream<Item = AiEvent> + Send>>;
}

pub enum AiEvent {
    TextDelta(String),        // Chat-Text zum sofortigen Anzeigen (Streaming)
    ActionProposed(AiAction), // strukturierter Vorschlag, s. Spec 0003 Abschnitt 5.2
    Done,
    Error(AiError),
}

pub struct SessionContext {
    pub system_context: String,     // effective_notes() aus Spec 0003 + OS/Distro-Info
    pub history: Vec<ChatMessage>,
    pub available_actions: Vec<ActionSchema>,
}

pub struct ChatMessage {
    pub role: Role,
    pub content: MessageContent,
}

pub enum Role { User, Assistant, ActionResult }

pub enum MessageContent {
    Text(String),
    CommandResult { command: String, output: CommandOutput }, // aus Spec 0005
}
```

`send()` liefert einen Stream statt einer einzelnen Antwort, damit
Chat-Text im UI wortweise erscheinen kann (Standard-UX bei Chat-Oberflächen)
und strukturierte Aktionsvorschläge trotzdem eindeutig als eigene Events
erkennbar bleiben, statt aus Freitext geparst werden zu müssen.

## 4. Tool-Calling-Strategie

Zwei Provider-Kategorien, unterschiedlich behandelt:

- **Natives Tool-/Function-Calling** (OpenAI, Anthropic, die meisten
  aktuellen Modelle): `ActionSchema` wird 1:1 auf das jeweilige
  Tool-Definitionsformat der Provider-API gemappt. Die Provider-Implementierung
  übersetzt die zurückgelieferten Tool-Calls direkt in `AiAction`-Werte.
  Das ist der bevorzugte, zuverlässigere Weg.
- **Fallback für Modelle ohne zuverlässiges Tool-Calling** (z. B. manche
  lokalen Ollama-Modelle): Prompt-Instruktion, Aktionsvorschläge als klar
  abgegrenzten JSON-Block in der Antwort zu liefern. Die Provider-Implementierung
  versucht, diesen Block zu parsen. **Scheitert das Parsen, wird der Text
  als normale `TextDelta`-Chatnachricht behandelt, nicht als Fehler** — die KI
  hat dann schlicht keinen strukturierten Vorschlag gemacht, das ist kein
  Sicherheitsproblem, weil ohnehin nichts ohne die Filter-Engine ausgeführt
  werden kann.

Diese Unterscheidung ist konfigurierbar pro Provider-Konfiguration (Flag
`supports_native_tool_calling: bool`), nicht automatisch erkannt — automatische
Erkennung wäre fehleranfällig und würde stillschweigend das falsche Verhalten
wählen können.

## 5. Redaction von Kommando-Output vor dem Versand

Bevor ein `CommandOutput` (Spec 0005) als `MessageContent::CommandResult` in
den Kontext für die nächste Anfrage aufgenommen wird, läuft er durch einen
Redactor:

```rust
pub trait OutputRedactor: Send + Sync {
    fn redact(&self, output: &CommandOutput) -> CommandOutput;
}
```

Default-Implementierung erkennt gängige Muster (private-Key-Blöcke,
`password=`/`token=`/`api_key=`-artige Zeilen, AWS-Key-Muster u. ä.) und
ersetzt sie durch einen Platzhalter (`[REDACTED]`), bevor der Output an einen
externen KI-Anbieter geht. Nutzer können zusätzliche eigene Regex-Muster in
den Einstellungen ergänzen (Speicherung dafür nicht Teil dieser Spec).
Wichtig: Redaction passiert **immer**, unabhängig vom gewählten Provider,
auch bei lokalen Modellen — Konsistenz ist hier wichtiger als die
theoretische Annahme, dass lokale Modelle "sicherer" seien.

## 6. Fehlerbehandlung

```rust
pub enum AiError {
    AuthenticationFailed,
    RateLimited,
    NetworkError(String),
    InvalidResponse(String),
    ContextTooLarge,
    ProviderUnavailable(String),
}
```

`ContextTooLarge` wird von der aufrufenden Seite (nicht in diesem Modul)
behandelt, indem ältere `history`-Einträge gekürzt werden, bevor erneut
gesendet wird — siehe offene Punkte, Abschnitt 8.

## 7. Testbarkeit

`MockAiProvider` unter `#[cfg(test)]` in `core::ai`, konfigurierbar mit einer
festen Sequenz von `AiEvent`s pro Aufruf. Damit lässt sich die komplette
Orchestrierungs-Logik (Vorschlag → Filter-Engine → SSH-Ausführung → Redaction
→ zurück in den Kontext) unabhängig von echten Provider-APIs testen. Tests
für die konkreten Provider-Implementierungen (`crates/ai-providers`) laufen
gegen einen lokalen Mock-HTTP-Server (z. B. `wiremock`), der die jeweilige
Provider-API simuliert — keine echten API-Calls in der Testsuite.

Testfälle (Auszug):
- Tool-Call-Response eines nativen Providers wird korrekt zu `AiAction`
  gemappt
- Fallback-JSON-Block wird korrekt geparst, wenn wohlgeformt
- Fallback-Parsing scheitert graceful bei fehlerhaftem JSON → Text statt
  Absturz
- Redactor maskiert bekannte Secret-Muster in `CommandOutput`, lässt
  restlichen Output unverändert
- `AiError::AuthenticationFailed` bei ungültigem API-Key wird korrekt
  durchgereicht, nicht stillschweigend verschluckt

## 8. Offene Punkte

- **Token-Budget/Kontext-Kürzung**: Wann und wie werden ältere
  `history`-Einträge gekürzt, bevor der Kontext zu groß wird? MVP-Tendenz:
  einfache zeichenbasierte Näherung statt exaktem, providerspezifischem
  Tokenizer, mit Kürzung der ältesten Nachrichten zuerst. Exakte Umsetzung
  noch offen.
- **Speicherung der Provider-Konfiguration** (welcher Provider aktiv ist,
  Modellwahl, Base-URL für generische Endpoints) — analog zur noch offenen
  Host-Key-Speicherung aus Spec 0005 als kleine Ergänzung zu Spec 0004
  vorgesehen, nicht Teil dieser Spec. Der API-Key selbst liegt in jedem Fall
  im `CredentialStore` (Spec 0003), nie in der SQLite-DB.
- Soll ein Nutzer mehrere Provider gleichzeitig aktiv haben und pro
  Nachricht wählen, oder ist "ein aktiver Provider pro Session" ausreichend
  fürs MVP?
