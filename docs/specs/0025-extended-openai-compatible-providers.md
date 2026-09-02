# Spec: Erweiterte OpenAI-kompatible Provider (OpenRouter, TEE-gehostet)

Status: Entwurf
Modul: Erweiterung `crates/ai-providers`, `crates/app-tauri`, `frontend/`
Abhängigkeiten: KI-Provider-Trait und -Verwaltung (Spec 0006/0007)

## 1. Ziel

Der bestehende `generic_openai_compatible`-Providertyp (Spec 0006/0007)
deckt technisch bereits OpenRouter und die meisten anderen
OpenAI-API-kompatiblen Anbieter ab. Diese Spec verbessert die **Nutzbarkeit**
für diese Anbieter (Modellauswahl statt Freitext, anbieterspezifische
Zusatz-Header) und ergänzt eine **informative, nicht-kryptografisch-
verifizierte** Anzeige für TEE-gehostete Anbieter.

## 2. Modell-Discovery

```
discover_models(config: AiProviderConfigInput) -> Result<Vec<String>, AiError>
```

Ruft `GET {base_url}/models` gemäß OpenAI-API-Konvention auf (Standard bei
praktisch allen kompatiblen Anbietern, inkl. OpenRouter, das darüber
Hunderte verfügbare Modelle listet) und liefert die Modell-IDs. Im
Provider-Formular (Spec 0007, Abschnitt 8.3) ersetzt das Modellfeld für
`generic_openai_compatible`/`openai`/`ollama` ein durchsuchbares
Dropdown statt eines reinen Freitextfelds — mit Fallback auf Freitext, falls
`discover_models` fehlschlägt (nicht jeder Anbieter unterstützt den
Endpunkt zuverlässig, das darf das Anlegen eines Providers nicht
blockieren).

## 3. Anbieterspezifische Zusatz-Header

Ergänzung zu `AiProviderConfigInput` (Spec 0007, Abschnitt 8.2):

```rust
pub struct AiProviderConfigInput {
    // ... bestehende Felder
    pub extra_headers: Vec<(String, String)>,
}
```

Generisch statt OpenRouter-spezifisch benannt, da mehrere Anbieter eigene
Header erwarten (OpenRouter z. B. optional `HTTP-Referer`/`X-Title` für die
eigene Nutzungsstatistik/Rangliste — kein Pflichtfeld, aber nützlich).
`OpenAiCompatibleProvider` (Spec 0006) hängt diese Header an jeden Request
an. Im UI: ein einfaches Key-Value-Listenfeld, ein-/ausblendbar hinter
"Erweitert", damit das Formular für den Normalfall nicht überladen wirkt.

## 4. TEE-gehostete Anbieter — Informationsanzeige

**Kein automatisierter kryptografischer Verifikationsmechanismus** — das
wäre hardware-/anbieterspezifisch (SGX, TDX, unterschiedliche
Attestierungsformate) und würde falsche Sicherheit suggerieren, wenn nicht
vollständig korrekt implementiert. Stattdessen:

```rust
pub struct AiProviderConfigInput {
    // ...
    pub attestation_url: Option<String>,
}
```

Optionales Feld im "Erweitert"-Bereich. Ist es gesetzt, ruft die App den
angegebenen Endpunkt beim Speichern und auf Wunsch erneut ab
(`fetch_attestation_info(provider_id) -> String`) und zeigt die **rohe
Antwort** in einem Read-only-Textblock im Provider-Formular an, mit einem
unmissverständlichen Hinweis:

> "Dieser Wert wird unverändert vom Anbieter abgerufen und **nicht** von
> Smart SSH kryptografisch geprüft. Zur eigenständigen Verifikation nutze
> das vom Hardware-/Anbieter bereitgestellte Prüfwerkzeug."

Kein Badge, kein grünes Häkchen, kein Wort wie "verifiziert" im UI — die
App bestätigt nichts, sie zeigt nur an, was der Anbieter selbst meldet.

## 5. Offene Punkte

- Eine spätere, echte Attestierungsprüfung (z. B. für konkret benannte,
  häufig genutzte TEE-Anbieter mit dokumentiertem Format) wäre denkbar,
  ist aber ein eigenständiges, deutlich größeres Thema — bewusst nicht Teil
  dieser Spec.
- Sollen entdeckte Modelle (Abschnitt 2) zusätzliche Metadaten zeigen
  (Preis pro Token, Kontextfenstergröße), sofern der Anbieter das über
  denselben oder einen weiteren Endpunkt liefert (OpenRouter tut das)?
  Naheliegende Erweiterung, aber nicht in dieser Spec — würde eine
  anbieterspezifische Zusatzschnittstelle brauchen, die über den generischen
  OpenAI-Standard hinausgeht.
