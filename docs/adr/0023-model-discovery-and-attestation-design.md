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

**6. Die Attestierungs-Antwort rendert in der Provider-**Liste**, nicht im
Provider-**Formular**, wie Spec 0025, Abschnitt 4 wörtlich sagt ("... zeigt
die rohe Antwort in einem Read-only-Textblock im Provider-Formular an").
Es gibt in dieser App kein eigenes Bearbeiten-Formular pro bereits
gespeichertem Provider (`AiProviderSettings.tsx` hat genau ein Formular:
das zum **Anlegen** eines neuen Providers; bestehende Provider erscheinen
nur als Einträge der Liste darunter, mit Aktionen wie Löschen und eben dem
"Attestierung abrufen"-Button). Der Abruf selbst ist aber unmissverständlich
an einen bereits gespeicherten, konkreten Provider gebunden (`fetch_
attestation_info(provider_id)`) — ein rein clientseitiges Feld im
Anlage-Formular hätte für einen noch nicht gespeicherten Entwurf gar keine
`provider_id`, gegen die der Abruf laufen könnte. Die Liste ist damit der
einzige Ort, an dem "dieser konkrete, gespeicherte Provider" und "seine
zuletzt abgerufene Attestierung" zusammen sinnvoll darstellbar sind — der
im Spec-Text unterstellte Formular-Kontext existiert für einen bereits
gespeicherten Provider architektonisch nicht. Der Text der Spec ist damit
in der Sache erfüllt (Read-only-Textblock, unveränderte Antwort, derselbe
Disclaimer-Wortlaut), nur der Ort weicht vom wörtlichen Spec-Text ab —
unabhängiger Review-Pass hat das als dokumentationswürdige, nicht als zu
behebende Abweichung eingestuft: Spec und Code sollen nicht stillschweigend
auseinanderlaufen, aber ein neues, redundantes Bearbeiten-Formular nur für
diese eine Anzeige zu bauen wäre unverhältnismäßiger Aufwand für keinen
echten Zusatznutzen.

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
