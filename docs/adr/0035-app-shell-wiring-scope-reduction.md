# 0035-app-shell-wiring-scope-reduction

## Status
Vorgeschlagen

## Kontext

Spec 0038, Abschnitt 2 skizziert `Wiring::community()` als parameterlose
Funktion, die u. a. eine feste Liste von `Arc<dyn AiProvider>` (via
`ai_providers::all_byok()`) und eine SQLite-basierte `Arc<dyn
PolicySource>` (via `SqlitePolicySource::new(..)`) unmittelbar zurückgibt.
Beim Umsetzen von Teil 1 zeigte sich: beide Beispiele setzen Zustand
voraus, der zum Zeitpunkt eines parameterlosen `Wiring::community()`-Aufrufs
schlicht nicht existiert.

**KI-Provider:** Diese Codebasis baut `AiProvider`-Instanzen nie als feste
Liste beim App-Start. `ai_provider_factory::build_ai_provider(...)`
konstruiert pro Server-Verbindung eine Instanz aus einem
nutzerhinterlegten API-Key (`SecretString`, aus `CredentialStore` gelesen,
s. `commands::connect`). Es gibt keine Funktion `ai_providers::all_byok()`
in `crates/ai-providers` — die Spec-Skizze benennt eine Funktion, die nie
existiert hat. Eine `Vec<Arc<dyn AiProvider>>` ohne API-Key wäre für diese
Codebasis ohnehin kein sinnvoller Typ: `AiProvider`-Instanzen sind hier
untrennbar an einen Credential-Zustand gebunden, nicht an einen
Provider*typ*.

**SQLite-Policy-Source:** `SqlitePolicySource` (aus Spec 0037, Teil 2 —
tatsächlich `SqlitePolicyStore`, das seither zusätzlich `PolicySource`
implementiert) hängt an der von `build_app_state()` erst zur Laufzeit
geöffneten `SqliteProfileStore`-Verbindung (`SqliteProfileStore::connect`,
async, via `tauri::async_runtime::block_on`). `Wiring::community()` müsste
diese Verbindung selbst öffnen, um `policy_sources` zu befüllen — das
widerspricht dem in Abschnitt 1 gewählten Bild eines synchron und ohne
Umgebungszustand aufrufbaren `Wiring::community()`.

## Entscheidung

`Wiring::community()` bleibt parameterlos, exakt wie skizziert — aber
`ai_providers` und `policy_sources` bleiben in dieser Funktion leere
`Vec`s. Nur `entitlements` wird tatsächlich aus dem bisher fest
verdrahteten Setup-Code gelöst und in `Wiring` verschoben — der einzige der
drei in Spec 0038 Abschnitt 1 genannten Fälle ("AI provider list, SQLite
policy source, entitlements"), der ohne zusätzlichen Datenbank-/
Credential-Zustand sauber lösbar ist.

`crates/app-shell/src/lib.rs`s `build_app_state()` öffnet die SQLite-
Verbindung und leitet `policy_store`/`ai_provider_store` weiterhin
unverändert direkt daraus ab (`profile_store.policy_store()`,
`profile_store.ai_provider_store()`) — **nicht** über
`wiring.policy_sources`/`CombinedPolicySource`. Das ist dieselbe,
bewusste Zurückhaltung wie bereits bei der Einführung von `PolicySource`/
`CombinedPolicySource` selbst (Spec 0037, Teil 2): die Typen existieren
und sind getestet, ihre produktive Verdrahtung in `AppState` wird
zurückgestellt, bis es tatsächlich eine zweite Policy-Quelle gibt (z. B.
eine künftige Organisations-Quelle im Official-Binary). Dasselbe gilt
jetzt für `ai_providers`: der Typ in `Wiring` existiert für ein künftiges
Binary, das tatsächlich eine feste Provider-Liste braucht (z. B. ein
Managed-Provider ohne nutzereigenen API-Key) — die Community-Edition
braucht ihn aktuell nicht.

`Wiring`/`Edition`/`Wiring::community()` selbst sind exakt nach Spec 0038
Abschnitt 2 typisiert (inklusive `sync_backends: vec![]`, `plugins: vec![]`
— beide bereits laut Spec-Text leer für Community). Die einzige Abweichung
vom Code-Beispiel ist der tatsächliche Inhalt von zwei der sechs Felder,
nicht die Struktur.

## Konsequenzen

**Positiv:**
- `Wiring::community()` bleibt parameterlos und ohne I/O — passt zum in
  Spec 0038 gezeichneten Bild eines reinen Konfigurations-Structs.
- Keine Vortäuschung von Funktionalität: eine nicht-leere, aber
  bedeutungslose Liste (z. B. `AiProvider`-Instanzen ohne echten API-Key)
  wäre irreführender als eine ehrlich leere.
- Konsistent mit dem bereits in Spec 0037 etablierten Muster
  ("Typ/Trait jetzt einführen, produktive Verdrahtung zurückstellen, bis
  ein zweiter Anwendungsfall existiert").

**Negativ / Trade-off:**
- `Wiring.ai_providers`/`Wiring.policy_sources` sind für die Community-
  Edition aktuell reine, unbefüllte Vokabeln — kein Verhalten hängt heute
  von ihrem Inhalt ab. Wer künftig eine zweite Policy-Quelle oder eine
  Managed-Provider-Liste verdrahten will, muss `build_app_state()`
  zusätzlich so ändern, dass es `wiring.policy_sources`/`wiring.ai_providers`
  tatsächlich liest (heute liest es sie nicht) — dieser Umbau ist bewusst
  nicht Teil dieses Schritts.
- `apps/smart-ssh-community/Cargo.toml` listet dieselben
  `tauri-plugin-*`-Abhängigkeiten wie `crates/app-shell/Cargo.toml` ein
  zweites Mal (s. Kommentar dort): `tauri-build`s ACL-/Capability-
  Auflösung in `build.rs` fand die Plugin-Berechtigungen über eine rein
  transitive Abhängigkeit (`smart-ssh-community` → `app-shell` →
  `tauri-plugin-dialog`) nachweislich nicht ("Permission dialog:default
  not found" beim Build) — reproduzierbar durch Weglassen der direkten
  Deklaration. Ein künftiges zweites Binary (Official) müsste dieselbe
  Liste ebenfalls direkt deklarieren.
