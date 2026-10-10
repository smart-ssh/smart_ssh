# Spec 0072 — Anthropic richtig behandeln: Fehler erkennen, Modelle laden

Status: umgesetzt
Zweck: Ein falscher Modellname führt bei jedem unterstützten Anbieter zur
Meldung „Modell nicht gefunden" (nicht „Anbieter nicht erreichbar"), und für
Anthropic ist der Knopf „Modelle laden" verfügbar.
Bezüge: Spec 0006 (Fehlerarten), Spec 0025 (Modell-Entdeckung), Spec 0056
(Anbieter-Formular), Spec 0024 (Sprache).

Review-Einstufung: normal (Fehlerabbildung und ein lesender Zugriff auf
einen Anbieter-Endpunkt; keine Sicherheits-Invariante berührt, Angriffs-
richtungen trotzdem in 6.3).

## Entscheidungen

- **Teil 1:** Die Erkennung stützt sich auf die **Struktur der Antwort**,
  nicht auf einen weiteren Textmarker. Die Markerliste bleibt als
  Auffangnetz.
- Die Fehlerbilder der übrigen Anbieter (OpenAI, OpenRouter, Ollama) sind
  nicht gegen echte Systeme gemessen; dort greift entweder das
  Struktur-Feld oder die Markerliste (Abschnitt 8).

## 1. Ausgangslage

Eine falsche Modell-ID liefert bei Anthropic HTTP 404 mit

```json
{"type":"error",
 "error":{"type":"not_found_error","message":"model: claude-9-does-not-exist"},
 "request_id":"req_…"}
```

Die Meldung enthält **keinen** der bisherigen Textmarker (`model_not_found`,
`does not exist or you do not have access`, `not found, try pulling`,
`is not a valid model id`, `model not found`); der Fehler wurde daher als
„Provider nicht erreichbar" gemeldet.

## 2. Ziel und Nicht-Ziele

**Ziel:** Ein falscher Modellname führt bei jedem unterstützten Anbieter zu
`AI_MODEL_NOT_FOUND`; wo der Anbieter ein strukturiertes Feld liefert,
zählt dieses Feld statt Prosa.

**Nicht-Ziele:**

1. Keine Anbieter-Fallunterscheidung bei der Abbildung des Statuscodes; die
   Struktur der Antwort genügt.
2. Die Markerliste wird nicht abgeschafft, nur entlastet.
3. Keine Änderung der Fehlerarten oder der Fehlercodes Richtung Frontend.

## 3. Anforderungen (Teil 1)

**A1 (MUSS)** Vor der Marker-Prüfung wird der Antwort-Körper als JSON
gelesen. Trifft eine dieser Formen zu, gilt „Modell nicht gefunden":

| Feld | Wert | Anbieter |
|---|---|---|
| `error.type` | `not_found_error` | Anthropic |
| `error.code` | `model_not_found` | OpenAI-kompatible |

**A2 (MUSS)** Greift keine Form — auch bei ungültigem JSON —, entscheidet
wie bisher die Markerliste. Kein Körperformat darf zu einem Fehler führen;
die Prüfung ist rein additiv.

**A3 (MUSS)** Die Statusbedingung bleibt: nur bei **404 oder 400**. Ein
`not_found_error` bei einem anderen Status bleibt „Provider nicht
erreichbar". 401/403 bleiben „Authentifizierung fehlgeschlagen", 429 bleibt
Rate-Limit; der Status wird zuerst geprüft.

**A4 (MUSS)** Die Test-Fixture für Anthropic enthält die gemessene Antwort.
Die Fixture-Beschreibung nennt je Anbieter, ob die Datei **gemessen** oder
**rekonstruiert** ist (mit Datum bei den gemessenen).

**A5 (MUSS)** Die Markerliste ist als Auffangnetz hinter A1 dokumentiert, und
ihre Einträge für OpenAI, OpenRouter und Ollama gelten als unbelegt.

## 4. Design

- **Struktur statt eines sechsten Markers.** `not_found_error` als
  Teilzeichenkette würde jeden 404 dieses Typs zu „Modell nicht gefunden"
  machen. Ein Feldvergleich sagt, was gemeint ist.
- **Restrisiko, benannt.** `error.type == not_found_error` ist bei Anthropic
  nicht auf Modelle beschränkt. Das ist unkritisch, solange die App nur
  Chat-Anfragen und die Modellliste aufruft; bei beiden ist die einzige vom
  Nutzer gesetzte Ressource der Modellname. Kommt ein Aufruf mit weiteren
  Ressourcen hinzu, ist die Annahme neu zu prüfen; sie steht als Kommentar
  an der Prüfstelle.
- **Verworfen:** den Anbietertyp durch die Statusabbildung zu reichen — das
  berührt jeden Aufrufer.

## 5. Sicherheitszusagen

Keine Sicherheits-Invariante berührt: Die Änderung entscheidet, welcher Text
angezeigt wird, nicht ob etwas ausgeführt wird. Zwei bestehende
Eigenschaften bleiben:

- **Kein Secret im Fehlertext.** Der Körper ist eine Anbieter-Antwort, kein
  Key; das redigierende Logging des Fehlerpfads bleibt unverändert.
- **Status vor Inhalt.** 401/403 werden nie durch einen Körpertext zu
  „Modell nicht gefunden".

## 6. Akzeptanzfälle

### 6.1 Struktur-Erkennung

- T1 Anthropic-Antwort (404, `error.type = not_found_error`) → Modell nicht
  gefunden (Regressionstest dieses Funds).
- T2 OpenAI-Antwort (404, `error.code = model_not_found`) → Modell nicht
  gefunden, über A1 wie über die Marker.
- T3 Ollama-Antwort (404, nur Prosa) → Modell nicht gefunden über die
  Marker (A2 kappt das Auffangnetz nicht).
- T4 Kein JSON (`<html>502 Bad Gateway</html>`, 404) → Provider nicht
  erreichbar, kein Absturz.
- T5 Leerer Körper, 404 → Provider nicht erreichbar.

### 6.2 Abgrenzung

- T6 `not_found_error` bei 500 → Provider nicht erreichbar (A3).
- T7 401 mit `not_found_error` im Körper → Authentifizierung
  fehlgeschlagen.
- T8 429 mit demselben Körper → Rate-Limit.

### 6.3 Adversariale Fälle

- X1 Anbieter-Text ist nicht vertrauenswürdig: Ein Körper mit
  `not_found_error` **und** einem Platzhalter-Secret im `message`-Feld
  taucht in keiner geloggten Zeile im Klartext auf.
- X2 Verschachteltes JSON (`{"error":{"error":{"type":"not_found_error"}}}`)
  greift **nicht**; geprüft wird genau `error.type` auf der ersten Ebene.
- X3 Sehr großer Körper: Die JSON-Prüfung zieht nicht unbegrenzt Speicher.
  Das Lesen des Fehlerkörpers ist zeitlich, nicht byteweise begrenzt (siehe
  Abschnitt 8).
- X4 Falscher Typ (`{"error":{"type":123}}`): kein Absturz, Rückfall auf die
  Marker.

---

## Teil 2 — Modell-Entdeckung für Anthropic

Die Anthropic-API bietet eine Modellliste (`GET /v1/models`). Der Knopf
„Modelle laden" gilt daher auch für den Typ Anthropic.

**B1 (MUSS)** Die Modell-Entdeckung wählt die Authentifizierung nach
Anbieter: `x-api-key` plus `anthropic-version` für Anthropic,
`Authorization: Bearer` für die OpenAI-kompatible Familie. Die
`anthropic-version` hat eine einzige Quelle, dieselbe wie der Chat-Pfad.

**B2 (MUSS)** Die Oberfläche bietet „Modelle laden" auch für Anthropic an.
Der Endpunkt ist für Anthropic `/v1/models` (die Basis-URL enthält dort
kein `/v1`), für die OpenAI-Familie `{Basis-URL}/models`.

**B3 (MUSS)** Zusatz-Header (Spec 0025, Abschnitt 3) überschreiben gesetzte
Header nicht still: Setzt der Nutzer dieselbe Kopfzeile, gilt seine, aber
genau eine.

**B4 (MUSS)** Ein falscher Key ergibt auch hier „Authentifizierung
fehlgeschlagen", nicht „keine Modelle".

Tests: Anthropic sendet `x-api-key` und `anthropic-version`, **kein**
`authorization` (B-T1); OpenAI-kompatibel unverändert `authorization:
Bearer` (B-T2); Entdeckung ist für `anthropic` angeboten, die übrigen Typen
unverändert (B-T3); 401 → Authentifizierung fehlgeschlagen, leere Liste ist
kein gültiges Ergebnis (B-T4); eine per Zusatz-Header gesetzte `x-api-key`
erscheint genau einmal (B-T5).

Nicht Teil dieser Spec: *wann* die Entdeckung ausgelöst wird. Sie läuft nur
auf Nutzeraktion (Knopf), nie automatisch im Hintergrund.

---

## Teil 3 — Platzhaltertext in der Verwaltungsansicht

**C1 (MUSS)** Der Platzhaltertext der Verwaltungsansicht kommt über
Sprachschlüssel (DE und EN), kein fest eingebauter String.

**C2 (MUSS)** Beide Sprachdateien enthalten den Schlüssel; die Prüfung auf
fehlende Gegenstücke bleibt grün.

---

## 8. Grenzen

- Die Fehlerbilder von OpenAI, OpenRouter und Ollama sind nicht gegen
  echte Systeme gemessen; ihre Fixtures sind rekonstruiert.
- Der Fehlerkörper wird zeitlich begrenzt gelesen, aber nicht auf eine
  Byte-Obergrenze gekürzt.
