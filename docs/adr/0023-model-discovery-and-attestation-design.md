# 0023-model-discovery-and-attestation-design

## Status
Vorgeschlagen

## Kontext

`docs/specs/0025-extended-openai-compatible-providers.md` gibt die
Command-Signaturen (`discover_models`, `fetch_attestation_info`) und die
UI-Anforderungen vor, lässt aber mehrere konkrete Umsetzungsfragen offen,
die beim Implementieren entschieden werden mussten.

## Entscheidungen

**1. Modell-Dropdown als natives `<input list>` + `<datalist>`, keine
eigene Combobox-Komponente.** Spec verlangt "durchsuchbares Dropdown ...
mit Fallback auf Freitext". Ein `<datalist>` ist strukturell immer ein
normales Texteingabefeld (der geforderte Fallback ist damit automatisch
erfüllt, nicht nachträglich implementiert) und bekommt Browser-native
Autocomplete/Filterung ohne zusätzliche Abhängigkeit. Kein Fokus-Management,
keine Tastatur-Navigation selbst bauen — der Browser übernimmt das.

**2. `discover_models`/`fetch_attestation_info` als freie Funktionen in
`crates/ai-providers`, nicht als Methoden auf `AiProvider`.** Der Trait ist
auf Chat-Streaming zugeschnitten (`send(context) -> Stream<AiEvent>`).
`discover_models` muss außerdem gegen einen noch nicht gespeicherten
Formular-Entwurf laufen (kein `AiProviderConfig`, keine `ProviderId`) — eine
Trait-Methode hätte entweder eine künstliche Dummy-Instanz oder eine
zweite Konstruktionsart gebraucht. Freie Funktionen mit primitiven
Parametern (`base_url`, `api_key`, `extra_headers`) sind das einfachere,
direkt wiremock-testbare Modell.

**3. `extra_headers` als JSON-kodierte `TEXT`-Spalte statt eigener Tabelle.**
Header gehören untrennbar zu genau einer Provider-Config, es gibt keinen
Anwendungsfall, sie einzeln zu adressieren oder über Provider hinweg zu
joinen — eine eigene Zeilen-pro-Header-Tabelle wäre reiner Overhead für
einen internen Konfigurationswert.

**4. `fetch_attestation_info` schickt bewusst kein `api_key`/keine
`extra_headers` des KI-Providers mit.** Ein Attestierungs-Endpunkt ist
konzeptionell ein eigenständiger, typischerweise unauthentifizierter
Nachweis-Dienst des Hardware-/Anbieters (unabhängig überprüfbar) — kein
Teil der Chat-API selbst. Ihm dieselben Zugangsdaten mitzugeben wäre eine
unbegründete Annahme über sein Schutzschema und würde den API-Key
potenziell an einen dritten, vom Nutzer nur lose verbundenen Endpunkt
weitergeben.

**5. `discover_models` lehnt `anthropic` explizit ab, statt einen
wahrscheinlich falsch geformten Request zu versuchen.** Spec 0025,
Abschnitt 2 nennt nur die OpenAI-kompatible Familie
(`openai`/`generic_openai_compatible`/`ollama`). Sowohl Frontend (Dropdown
nur für diese drei Typen) als auch Backend (Command validiert
`provider_type`) setzen das durch — Verteidigung in der Tiefe statt
Verlass auf nur eine Schicht.

## Konsequenzen

**Positiv:**
- Kein Blockieren des Provider-Formulars, wenn ein Anbieter `/models` nicht
  unterstützt — der Fallback ist strukturell garantiert, nicht nur getestet.
- Attestierungs-Anzeige transportiert keine Zugangsdaten an einen Endpunkt,
  dessen Vertrauenswürdigkeit die App nicht kennt.

**Negativ / Trade-off:**
- Ein Attestierungs-Endpunkt, der tatsächlich Authentifizierung verlangt,
  wird aktuell nicht unterstützt — dafür bräuchte es ein eigenes,
  separates Zugangsdaten-Feld für die Attestierung selbst (nicht Teil
  dieser Spec, offener Punkt für eine spätere Erweiterung).
- Kein Preis-/Kontextfenster-Metadaten im Dropdown (OpenRouter würde das
  über einen weiteren Endpunkt liefern) — bewusst nicht gebaut, Spec 0025
  nennt das explizit als offenen Punkt, nicht als Anforderung dieser Spec.
