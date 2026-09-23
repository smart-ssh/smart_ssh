# ADR XXXX: Entscheidungen bei der Umsetzung von Spec 0072 (Anthropic: strukturierte Fehlererkennung, Modell-Discovery, Locale-Fix)

Status: Angenommen
Bezug: docs/specs/0072-model-not-found-structured-detection.md, Commits
`978e091` (Teil 1), `395095c` (Teil 2), `d0d14f2` (Teil 3), `7ba6aeb`
(Review-Nacharbeit Runde 1), `8597245` (Review-Nacharbeit Runde 2)

Diese Spec verlangt kein eigenes ADR für eine offene Design-Entscheidung
(§4 begründet die zentrale Design-Wahl — Struktur statt Textmarker — bereits
selbst). Laut Coder-Arbeitsweise gehört hierher trotzdem: jede beim
Abschluss-Review gefundene, aber bewusst nicht behobene Stelle, und jede
Abweichung von einer wörtlichen Spec-Annahme.

## 1. `effective_headers` dedupliziert nur gegen die Default-Header, nicht `extra_headers` untereinander

**Frage:** B3 verlangt, dass ein per `extra_headers` gesetzter Header
denselben internen Default-Header (`x-api-key`/`anthropic-version`/
`authorization`) ersetzt, statt doppelt gesendet zu werden. Was passiert,
wenn ein Nutzer *innerhalb* von `extra_headers` selbst denselben
Header-Namen zweimal einträgt?

**Entscheidung:** Nur gegen die ursprünglich gesetzten Default-Einträge
deduplizieren (`headers[..default_len]`), nicht gegen bereits durch
`extra_headers` selbst hinzugefügte Einträge.

**Begründung:** B3s Wortlaut ("setzt ein Nutzer dort dieselbe Kopfzeile,
gilt seine") spricht ausdrücklich vom Verhältnis Nutzer-Header zu
internem Default, nicht von zwei Nutzer-Einträgen zueinander. Eine erste
Fassung deduplizierte versehentlich auch Letzteres (Spec-Reviewer-Fund,
Runde 1) — stillschweigend, ohne dass das irgendwo verlangt oder auch nur
erwähnt war. Vor dieser Spec wurden zwei gleichnamige `extra_headers`-
Einträge beide gesendet (einfache `.header()`-Kette); das bleibt so.

**Konsequenz:** Getestet durch
`test_effective_headers_overrides_only_the_matching_default_not_other_extra_entries`.
Ob zwei gleichnamige Nutzer-Header überhaupt ein sinnvoller Anwendungsfall
sind (manche Gateways akzeptieren das), ist nicht Teil dieser Spec.

## 2. Bewusst nicht behoben: `/v1`-Doppelpräfix bei einer selbst eingetragenen Anthropic-`base_url`

**Fund (Review Runde 2):** Der Discovery-Pfad baut für Anthropic jetzt
`{base_url}/v1/models` (Fix für den in Runde 1 gefundenen Blocker, s.
unten). Trägt ein Nutzer selbst eine `base_url` **mit** `/v1`-Suffix ein
(z. B. ein Proxy unter `https://proxy.example/v1`), ergäbe das
`.../v1/v1/models`.

**Nicht behoben, weil:** exakt symmetrisch zum seit Langem bestehenden
Chat-Pfad (`crate::anthropic`, `format!("{}/v1/messages", ...)`) — kein
neues, durch diese Spec eingeführtes Verhalten, sondern ein vorbestehender
Fall, der für Anthropic bislang nur deshalb nicht sichtbar war, weil das
`base_url`-Feld für `anthropic` im Formular gar nicht angezeigt wird
(`needsBaseUrl` deckt nur `generic_openai_compatible`/`ollama` ab) — ein
Nutzer kann diesen Wert nur über einen Formular-Zustand erreichen, der von
einem anderen Provider-Typ übrig geblieben ist (`AiProviderSettings.tsx`
löscht `baseUrl` beim Typ-Wechsel nicht). Eine `/v1`-Normalisierung würde
beide Pfade gemeinsam betreffen und ist eine Entscheidung über das
Formularverhalten aller vier Provider-Typen, nicht nur über Teil 2 dieser
Spec — außerhalb des hier freigegebenen Dateisatzes.

**Vorschlag fürs Backlog:** entweder `base_url` beim Wechsel des
Provider-Typs im Formular zurücksetzen (analog zum bestehenden
Ollama-Vorbelegungsverhalten, Spec 0069 Teil B5), oder eine gemeinsame
`/v1`-Normalisierung in `ai-providers` vor dem Anhängen von
`/v1/messages`/`/v1/models`.

## 3. Bewusst nicht behoben: `sensitive`-Header-Markierung bleibt auf den Discovery-Pfad beschränkt

**Fund (Review Runde 1):** Der API-Key verlor beim Wechsel von
`bearer_auth(...)` auf einen einfachen `.header(name, value)`-Aufruf
`reqwest`s `sensitive`-Markierung (kein Klartext in `Debug`-Ausgaben,
keine HTTP/2-HPACK-Indizierung). Behoben für den **Discovery**-Pfad
(`effective_headers`/`sensitive_header_value`/`is_secret_header_name`,
`discovery.rs`).

**Nicht (zusätzlich) behoben:** derselbe fehlende Schutz besteht seit
Langem auch im **Chat**-Pfad (`crate::anthropic::connect_and_stream`,
`x-api-key` ohne `set_sensitive`) sowie im Chat-Pfad der OpenAI-Familie
für `extra_headers` (`crate::openai_compatible`, dort zusätzlich derselbe
Doppel-Header-Bug wie vor dieser Spec im Discovery-Pfad: `extra_headers`
werden per Kette aus `.header(...)`-Aufrufen angehängt statt einen
gleichnamigen `bearer_auth`/`accept`-Header zu ersetzen).

**Warum nicht mit erledigt:** außerhalb des von der Spec freigegebenen
Dateisatzes (nur `discovery.rs`/`app-shell`-Discovery-Command/Frontend);
der Chat-Pfad ist der sicherheitsrelevantere, stärker frequentierte Code
und verdient eine eigene, dedizierte Änderung mit eigenem Review statt
eines Nebenprodukts dieser Spec.

**Vorschlag fürs Backlog:** `sensitive_header_value`/`is_secret_header_name`
(oder eine gemeinsame Verallgemeinerung, inkl. weiterer secret-tragender
Header-Namen wie `api-key`/`x-goog-api-key` für künftige Provider) auch im
Chat-Pfad beider Provider anwenden; dort gleichzeitig den
Doppel-Header-Bug für `extra_headers` beheben (`effective_headers`-Muster
wiederverwenden statt zu duplizieren).

## 4. Bewusst nicht behoben: kein Byte-Cap auf Fehler-Antwort-Bodies

**Fund (Review Runde 1):** Ein Code-Kommentar behauptete fälschlich einen
bestehenden Größen-Cap auf `read_error_body_with_timeout`
(`crate::sse`) — tatsächlich existiert dort nur ein **Zeit**-Timeout, kein
Byte-Limit. Der Kommentar (und die identische Annahme in Spec 0072 §6.3,
X3) ist korrigiert (s. `docs/specs/0072-...md` §9, `error.rs`).

**Nicht behoben:** der fehlende Byte-Cap selbst. Vorbestehende Lücke,
durch diesen Schritt nicht verschärft (der neue `serde_json`-Parse-Schritt
in `is_structured_model_not_found` erhöht den Peak-Speicherverbrauch bei
einem sehr großen Body zusätzlich, ändert aber nichts an der fehlenden
oberen Schranke selbst).

**Vorschlag fürs Backlog:** `read_error_body_with_timeout` zusätzlich auf
eine feste Byte-Grenze (z. B. 64 KiB) deckeln, mit einer Kennzeichnung
("gekürzt") im geloggten/angezeigten Text.

## 5. Bewusst nicht behoben: kein `app-shell`-Command-Test für B2

**Fund (Review, beide Runden):** `crates/app-shell/src/commands::
discover_models` wählt seit Teil 2 den provider-abhängigen Default-
Base-URL (`DEFAULT_ANTHROPIC_BASE_URL` vs. `DEFAULT_OPENAI_BASE_URL`) und
lehnt `anthropic` nicht mehr ab — dafür existiert kein dedizierter Test auf
Command-Ebene, nur die (jetzt korrekten) Tests in `ai-providers::
discovery` und der manuelle Testablauf im Abschlussbericht. Ein
Doc-Kommentar, der fälschlich auf einen solchen Test verwies, ist
korrigiert (Review Runde 2).

**Warum nicht ergänzt:** `discover_models` als `#[tauri::command]` braucht
ein vollständiges `AppState` (Store/Keychain), dessen Testaufbau in diesem
Modul bislang nicht existiert (andere Commands hier sind ebenfalls ohne
dedizierten Unit-Test, s. `credential_test_tests` als bisher einzige
Ausnahme). Ein neues Test-Harness dafür aufzubauen wäre ein eigener,
größerer Schritt, keine Nebenwirkung dieser Spec.

**Vorschlag fürs Backlog:** die Default-Base-URL-Auswahl
(`config.provider_type` → `DEFAULT_ANTHROPIC_BASE_URL`/
`DEFAULT_OPENAI_BASE_URL`) in eine eigene, ohne `AppState` testbare
Funktion ziehen (Muster wie das bestehende `missing_required_base_url`)
statt sie inline im Command zu belassen.

## 6. `CHANGELOG.md`: kein direkter Eintrag (Projektkonvention, kein offener Punkt)

Der erste Review-Durchlauf bemängelte einen fehlenden `CHANGELOG.md`-
Eintrag für die drei nutzersichtbaren Änderungen. Das ist nach
`CLAUDE.md` ("Git staging"/"Versioning & changelog") kein offener Punkt:
Changelog-Fragmente kommen nach `changelog.d/<spec-nummer>-<thema>.md`,
nicht direkt in `CHANGELOG.md` — genau das liegt als
`changelog.d/0072-model-not-found-structured-detection.md` vor.
