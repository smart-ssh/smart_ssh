# Spec 0072 — Anthropic richtig behandeln: Fehler erkennen, Modelle laden

Status: freigegeben (Stefan, 2026-09-23) · Backlog: BL-0200, BL-0215, BL-0202 ·
Gate: Release-Gate 1.0, B
Repo: **öffentlich** `smart-ssh` — `crates/ai-providers/src/{error,discovery}.rs`,
`crates/ai-providers/tests/fixtures/model_not_found/`, Frontend
(`src/types.ts`, `AiProviderSettings.tsx`, `ManagementView.tsx`,
`locales/{de,en}/common.json`)
Review-Priorität: normal (Fehlerabbildung und ein Lesezugriff auf einen
Anbieter-Endpunkt; keine Sicherheits-Invariante — Angriffsrichtungen
trotzdem in §6.3)

> Zwei Stellen behandeln Anthropic anders als die übrigen Anbieter, und
> **beide aus derselben überholten Annahme**: Die API könne weniger, als
> sie kann. Ein vertippter Modellname meldet „Provider nicht erreichbar"
> statt „Modell nicht gefunden" (Teil 1), und der Knopf „Modelle laden"
> fehlt ganz (Teil 2). Teil 3 ist eine einzeilige Übersetzung, die
> mitläuft, weil sie keinen eigenen Lauf rechtfertigt.

## Getroffene Entscheidungen (Stefan, 2026-09-23)

- **Teil 1:** Erkennung **anhand der Antwortstruktur**, nicht durch einen
  weiteren Textmarker. Die Markerliste bleibt als Auffangnetz bestehen.
- **OP-1 (unten): (a)** — jetzt umsetzen, die Messung der drei übrigen
  Anbieter bleibt an BL-0200 hängen.

---

## 1. Ausgangslage (gemessen, nicht angenommen)

`map_http_status` (`error.rs:54`) bildet heute ab:

```rust
404 | 400 if contains_model_not_found_marker(body) => AiError::ModelNotFound(…)
_                                                   => AiError::ProviderUnavailable(…)
```

`contains_model_not_found_marker` sucht fünf Teilzeichenketten
(`error.rs:31`): `model_not_found`, `does not exist or you do not have
access`, `not found, try pulling`, `is not a valid model id`,
`model not found`.

**Messung am 2026-09-23**, echter Aufruf gegen `POST /v1/messages` mit
falschem Modellnamen, HTTP 404:

```json
{"type":"error",
 "error":{"type":"not_found_error","message":"model: claude-9-does-not-exist"},
 "request_id":"req_…"}
```

Die Fixture führte `"message": "model not found: …"`; tatsächlich steht
dort **nur** `"model: …"`. Keiner der fünf Marker trifft → der Fehler
landet auf `ProviderUnavailable`. Die Fixture
`tests/fixtures/model_not_found/anthropic.json` trägt inzwischen die echte
Antwort (im Arbeitsbaum, noch nicht committet).

**Ungemessen geblieben:** OpenAI, OpenRouter, Ollama — beim Messlauf
fehlten Key bzw. laufende Instanz. Ihre Fixtures sind weiterhin
rekonstruiert, ihre Marker damit unbelegt.

## 2. Ziel und Nicht-Ziele

**Ziel:** Ein falscher Modellname führt bei jedem unterstützten Anbieter zu
`AI_MODEL_NOT_FOUND`, und die Erkennung stützt sich dort, wo der Anbieter
ein strukturiertes Feld liefert, auf dieses Feld statt auf Prosa.

**Nicht-Ziele:**

1. **Keine Anbieter-Fallunterscheidung in `map_http_status`.** Die Funktion
   bekommt heute nur Status und Body; einen Anbietertyp durchzureichen
   berührt jeden Aufrufer. Die Struktur der Antwort genügt.
2. **Die Markerliste wird nicht abgeschafft**, nur entlastet.
3. **Keine Änderung an `AiError` oder an den Fehlercodes** Richtung
   Frontend. `AI_MODEL_NOT_FOUND` existiert und wird übersetzt.
4. Nicht Teil dieser Spec: die ungemessenen Fixtures für OpenAI,
   OpenRouter und Ollama (§8, OP-1).

## 3. Anforderungen

**A1 (MUSS)** Vor der Marker-Prüfung wird der Body als JSON gelesen. Trifft
eine der folgenden Formen zu, gilt `ModelNotFound`:

| Feld | Wert | Anbieter |
|---|---|---|
| `error.type` | `not_found_error` | Anthropic |
| `error.code` | `model_not_found` | OpenAI-kompatible |

**A2 (MUSS)** Greift keine Form — auch bei ungültigem JSON —, entscheidet
wie bisher die Markerliste. Kein Body-Format darf zu einem Fehler führen;
die Prüfung ist rein additiv.

**A3 (MUSS)** Die Statusbedingung bleibt unverändert: nur bei **404 oder
400**. Ein `not_found_error` bei einem anderen Status bleibt
`ProviderUnavailable`. 401/403 bleiben `AuthenticationFailed`, 429 bleibt
`RateLimited` — die Prüfreihenfolge nimmt weiterhin den Status zuerst.

**A4 (MUSS)** `anthropic.json` enthält die gemessene Antwort. Die
`README.md` des Fixture-Verzeichnisses sagt je Anbieter, ob die Datei
**gemessen** oder **rekonstruiert** ist, mit Datum bei den gemessenen.

**A5 (MUSS)** Der Kommentar an `MODEL_NOT_FOUND_MARKERS` sagt, dass die
Liste das Auffangnetz hinter A1 ist und dass ihre Einträge für OpenAI,
OpenRouter und Ollama unbelegt sind.

## 4. Design

### 4.1 Warum Struktur statt eines sechsten Markers

`"not_found_error"` als Teilzeichenkette aufzunehmen wäre eine Zeile. Es
würde aber **jeden** 404 mit diesem Fehlertyp zu „Modell nicht gefunden"
machen, auch einen, der etwas anderes meint. Ein Feldvergleich sagt, was
gemeint ist; eine Teilzeichenkette sagt nur, was irgendwo im Text steht.

Dasselbe Argument gilt gegen `"model: "` — das trifft jede Fehlermeldung,
die das Wort beiläufig verwendet.

### 4.2 Restrisiko, benannt

`error.type == "not_found_error"` ist bei Anthropic nicht auf Modelle
beschränkt; ein unbekannter Pfad oder eine unbekannte Ressourcen-ID
erzeugt denselben Typ. Innerhalb dieser Crate ist das unkritisch:
`ai-providers` ruft nur `POST /v1/messages` und `GET /v1/models` auf, und
bei beiden ist die einzige vom Nutzer gesetzte Ressource der Modellname.
Kommt später ein Aufruf mit weiteren Ressourcen hinzu, ist diese Annahme
neu zu prüfen — deshalb steht sie als Kommentar an der Prüfstelle.

### 4.3 Verworfen

Den Anbietertyp durch `map_http_status` zu reichen: löst dasselbe Problem,
berührt aber jeden Aufrufer und macht eine bisher reine Funktion vom
Aufrufkontext abhängig.

## 5. Sicherheits-Invarianten

**Keine berührt** — begründet: Die Änderung entscheidet, **welcher Text**
einem Nutzer angezeigt wird, nicht ob etwas ausgeführt wird. Sie berührt
weder Filter-Engine, Risiko-Klassifizierer, Redactor noch den
Ausführungspfad.

Zwei bestehende Eigenschaften bleiben und werden getestet:

- **Kein Secret im Fehlertext.** `ModelNotFound` trägt `HTTP {status}:
  {body}` — der Body ist eine Anbieter-Antwort, kein Key. Die
  Redaction-Prüfung auf diesem Pfad (`log_provider_error_response`) bleibt
  unverändert.
- **Status vor Inhalt.** 401/403 dürfen nicht durch einen Körpertext zu
  `ModelNotFound` werden.

## 6. Tests

### 6.1 Struktur-Erkennung

- T1 Anthropic-Fixture (404, `error.type = not_found_error`) →
  `ModelNotFound`. **Scheitert am heutigen Stand** — das ist der
  Regressionstest für diesen Fund.
- T2 OpenAI-Fixture (404, `error.code = model_not_found`) →
  `ModelNotFound`, sowohl über A1 als auch über die Marker.
- T3 Ollama-Fixture (404, nur Prosa) → `ModelNotFound` über die Marker;
  belegt, dass A2 das Auffangnetz nicht kappt.
- T4 Kein JSON (`"<html>502 Bad Gateway</html>"`, 404) →
  `ProviderUnavailable`, kein Absturz.
- T5 Leerer Body, 404 → `ProviderUnavailable`.

### 6.2 Abgrenzung

- T6 `not_found_error` bei **500** → `ProviderUnavailable` (A3).
- T7 401 mit `"error":{"type":"not_found_error"}` im Body →
  `AuthenticationFailed`, nicht `ModelNotFound`.
- T8 429 mit demselben Body → `RateLimited`.

### 6.3 Adversariale Fälle

- X1 **Anbieter-Text ist nicht vertrauenswürdig.** Ein Body mit
  `{"error":{"type":"not_found_error"}}` **und** einem Platzhalter-Secret
  (`sk-live-hunter2`) im `message`-Feld: Der Fehlerwert darf entstehen,
  aber der Platzhalter darf in keiner geloggten Zeile auftauchen
  (bestehende Redaction).
- X2 **Tief verschachteltes JSON** (`{"error":{"error":{"type":
  "not_found_error"}}}`) → greift **nicht**; geprüft wird genau
  `error.type` auf der ersten Ebene, nicht „irgendwo im Baum".
- X3 **Sehr großer Body** (mehrere MB): Die JSON-Prüfung darf nicht
  unbegrenzt Speicher ziehen — der bestehende Lese-Cap auf dem Fehlerpfad
  (`read_error_body_with_timeout`) bleibt davor.
- X4 **Falscher Typ:** `{"error":{"type":123}}` → kein Absturz,
  Rückfall auf die Marker.

## 7. Umsetzungsreihenfolge (Teil 1; Teil 2 und 3 danach)

1. Fixture `anthropic.json` mit der gemessenen Antwort committen, README
   nach A4 (gemessen/rekonstruiert je Anbieter).
2. T1 als roten Test schreiben — er muss gegen den heutigen Stand
   fehlschlagen, sonst prüft er nichts.
3. A1/A2 umsetzen, Kommentar nach A5 und §4.2.
4. T2–T8 und X1–X4 ergänzen.
5. Teil 2: B1/B2 mit B-T1 zuerst rot, dann grün; Kommentare berichtigen.
6. Teil 3: Locale-Schlüssel, beide Sprachen.
7. Gate grün fahren (`&&`-verkettet, nie in einer Pipe, Rückgabewert
   lesen — `RUST_EXIT=0` **und** `FE_EXIT=0`).

---

## Teil 2 — Modell-Erkennung für Anthropic (BL-0215)

### 1. Ausgangslage

`supportsModelDiscovery` (`src/types.ts:101`) lässt `anthropic` aus, mit
der Begründung: *„`discover_models` funktioniert nur gegen die
OpenAI-kompatible Familie (`GET {base_url}/models`) — `anthropic` hat kein
äquivalentes Endpoint-Verhalten."* (Spec 0025, Abschnitt 2) Der Knopf
„Modelle laden" erscheint dort deshalb gar nicht.

Die Annahme trifft nicht zu: Die Anthropic-API hat `GET /v1/models` (und
`GET /v1/models/{id}`), ohne Beta-Header, mit `data[]` aus `id`,
`display_name`, `created_at` und weiteren Feldern.

Der bestehende Pfad passt fast vollständig — geprüft in `discovery.rs`:

| | |
|---|---|
| URL | `discover_models_within` baut `{base_url}/models`; mit Anthropics Basis-URL ergibt das genau den Endpunkt |
| Antwortform | `ModelsResponse { data: Vec<ModelEntry { id }> }` entspricht der Antwort |
| **Was fehlt** | die Kopfzeilen: `discovery.rs:72` setzt `bearer_auth(api_key)`; Anthropic verlangt `x-api-key` und `anthropic-version` |

### 2. Anforderungen

**B1 (MUSS)** `discover_models` wählt die Authentifizierungs-Kopfzeilen
nach Anbieter: `x-api-key: <key>` plus `anthropic-version: <version>` für
Anthropic, `Authorization: Bearer <key>` für die OpenAI-kompatible
Familie. Die `anthropic-version` ist als Konstante zu führen, nicht als
Zeichenkette an der Aufrufstelle — sie steht bereits im Chat-Pfad
(`anthropic.rs`) und darf nicht zweimal auseinanderdriften (eine Quelle
der Wahrheit).

**B2 (MUSS)** `supportsModelDiscovery` nimmt `anthropic` auf. Der
überholte Kommentar dort und der in `discovery.rs` werden berichtigt —
stehen lassen wäre die nächste Falle für den Nächsten.

**B3 (MUSS)** `extra_headers` überschreiben die gesetzten Kopfzeilen nicht
still: Setzt ein Nutzer dort dieselbe Kopfzeile, gilt seine — aber genau
eine, nicht beide.

**B4 (MUSS)** Ein falscher Key ergibt auch hier `AuthenticationFailed`,
nicht „keine Modelle". Der bestehende Pfad über `map_http_status` bleibt.

### 3. Tests

- B-T1 Anthropic: Kopfzeilen enthalten `x-api-key` und
  `anthropic-version`, **kein** `authorization`. Scheitert am heutigen
  Stand.
- B-T2 OpenAI-kompatibel: unverändert `authorization: Bearer`.
- B-T3 `supportsModelDiscovery("anthropic") === true`; die übrigen
  Anbietertypen unverändert.
- B-T4 401 im Discovery-Pfad → `AuthenticationFailed`, leere Liste ist
  kein gültiges Ergebnis.
- B-T5 Eine per `extra_headers` gesetzte `x-api-key` erscheint genau
  einmal in der Anfrage.

**Nicht Teil dieser Spec:** *wann* die Erkennung ausgelöst wird (Knopf
gegen automatisch nach Key-Eingabe). Das ist BL-0220 und berührt die
Invariante „kein Netzwerkaufruf ohne Nutzeraktion" (BL-0042).

---

## Teil 3 — Platzhaltertext in `ManagementView` übersetzen (BL-0202)

Der Platzhaltertext in `ManagementView.tsx` ist fest deutsch und erscheint
so auch in der englischen Oberfläche. Er lag außerhalb des Dateisatzes von
Spec 0069.

**C1 (MUSS)** Text über Locale-Schlüssel in `locales/de/common.json` und
`locales/en/common.json`, kein fest eingebauter String mehr.
**C2 (MUSS)** Beide Sprachdateien bekommen den Schlüssel; ein Test oder
Lint, der fehlende Gegenstücke findet, bleibt grün.

**Warum das hier mitläuft und keine eigene Spec bekommt:** Es ist eine
Zeile. Ein eigener Coder-Lauf dafür kostet ein Vielfaches des Nutzens, und
Teil 2 fasst ohnehin dieselben Sprachdateien an.

---

## 8. Offene Punkte (Entscheidung Stefan)

### OP-1 — Die drei ungemessenen Anbieter

OpenAI, OpenRouter und Ollama sind weiterhin rekonstruiert. Nach dieser
Spec greift bei ihnen entweder `error.code` (OpenAI-kompatible) oder die
Markerliste — beides unbelegt.

- **(a) Diese Spec jetzt umsetzen, Messung nachziehen (Empfehlung).** Der
  Anthropic-Fall ist der belegte, und er ist der häufigste. Die Messung
  der übrigen drei bleibt an BL-0200 hängen, bis Key bzw. Ollama-Instanz
  verfügbar sind.
- **(b) Warten, bis alle vier gemessen sind.** Vollständiger, hält aber
  eine belegte Korrektur auf.

> **Entscheidung (Stefan, 2026-09-23): (a).** BL-0200 bleibt offen, bis
> OpenAI, OpenRouter und Ollama gemessen sind; diese Spec schließt den
> belegten Teil.

## 9. Klarstellungen

*(wird während der Umsetzung nachgetragen: Datum · Frage-ID · Antwort)*

- **2026-09-23, spec-reviewer (Review des Umsetzungsschritts):** zwei
  Annahmen dieser Spec trafen nicht zu, gemessen statt weiter angenommen:
  - **Teil 2, §1 „URL":** „mit Anthropics Basis-URL ergibt das genau den
    Endpunkt" — trifft nicht zu. Anthropics `base_url`
    (`DEFAULT_ANTHROPIC_BASE_URL`) enthält, anders als die der OpenAI-
    Familie, kein `/v1`-Präfix; `{base_url}/models` traf entsprechend
    `https://api.anthropic.com/models` (gemessen: HTTP 404) statt
    `/v1/models` (gemessen: HTTP 401 ohne Key, also der richtige Endpunkt).
    Behoben in `discovery.rs`: `/v1/models` nur für `ProviderType::
    Anthropic`, `/models` unverändert für die OpenAI-Familie.
  - **§6.3, X3:** „der bestehende Lese-Cap auf dem Fehlerpfad
    (`read_error_body_with_timeout`) bleibt davor" — es gibt dort keinen
    Byte-Cap, nur einen Zeit-Timeout. Nicht Teil dieses Schritts behoben
    (Backlog-Hinweis für einen künftigen Schritt), aber der entsprechende
    Code-Kommentar in `error.rs` behauptet es nicht mehr fälschlich.
