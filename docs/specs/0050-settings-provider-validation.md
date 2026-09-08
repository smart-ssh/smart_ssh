# Spec: Settings-Neustruktur & Provider-Key-Validierung

Status: Entwurf
Repo: **öffentlich** `smart_ssh` (Settings-Gerüst, Provider-Formular,
Registry-Anbindung)
Abhängigkeiten: Extension-Registry (0038, `registerSettingsSection`),
KI-Provider (0006/0007/0025), Server-Verbindungstest (0008 — Muster für den
Testen-Button)

> Drei zusammengehörige Frontend-Themen im Einstellungsbereich, gebündelt
> weil sie denselben Code betreffen. **Priorität NORMAL.** Voraussetzung:
> das Windows-Politur-Paket (0049, u. a. der Credential-Trim im
> Provider-Formular) ist durch — diese Spec baut auf dem getrimmten Stand
> auf.

## Teil 1: Zweispaltige Settings-Struktur

Der Settings-Screen ist aktuell eine **ungegliederte lange Liste**. Umbau auf
ein **zweispaltiges Layout**: Navigation links (Kategorien), Inhalt rechts
(die gewählte Kategorie) — wie die System-Einstellungen von macOS/Windows.

### 1.1 Kategorien (linke Navigation)
Sinnvolle Gruppierung der bestehenden Einstellungen, z. B.:
- **KI-Provider** (Anbieter hinzufügen/bearbeiten, Modell-Wahl,
  Zweitmeinungs-Provider)
- **Filter & Regeln** (Regel-Verwaltung, Test-Panel)
- **Anzeige & Sprache** (i18n, Theme falls vorhanden)
- **Sitzungen & Daten** (Aufbewahrung, Verschlüsselung-Info)
- **MCP-Server**
- **Lizenz** (falls Official — siehe 1.3)
- **Über / Erststart-Info**
Die konkrete Zuordnung darfst du sinnvoll wählen — beschreibe mir deine
Gruppierung, bevor du sie festzurrst.

### 1.2 KRITISCH — registrierte Sektionen aufnehmen
Die neue Struktur **muss** die über `registerSettingsSection` (Spec 0038)
registrierten Sektionen rendern, nicht nur die fest eingebauten. Das ist der
**einzige tatsächlich gerenderte** Registry-Kontributionspunkt — die
Official Edition klinkt ihre **Lizenz-Sektion** genau darüber ein. Wenn die
neue zweispaltige Navigation nur hartcodierte Kategorien kennt, **verschwindet
die Pro-Lizenz-Sektion**. Die registrierten Sektionen müssen als
Navigationseinträge (links) erscheinen und ihren Inhalt (rechts) rendern —
gleichwertig zu den eingebauten.

### 1.3 Zwei-Repo-Hinweis
Die Registry ist öffentlich, aber die Lizenz-Sektion wird **privat** (Official)
registriert. Prüfe: Reicht es, dass die öffentliche Settings-Struktur
registrierte Sektionen generisch aufnimmt (dann ist es rein öffentlich)? Oder
braucht die private Seite eine Anpassung, weil sich die Registrierungs-API
oder das Rendering ändert? **Melde mir das** — wenn die private Lizenz-Sektion
nach dem Umbau nicht mehr korrekt erscheint, ist das ein Zwei-Repo-Vorgang
(öffentliche Struktur + privater Folgeschritt), kein rein öffentlicher.

## Teil 2: Format-Hinweis für Provider-API-Keys (Stufe 1)

Im (jetzt neu strukturierten) KI-Provider-Formular: **sofortiger, offline
Format-Hinweis** beim Eingeben eines API-Keys.

- Präfix-Prüfung gegen den gewählten Provider: Anthropic `sk-ant-`, OpenAI
  `sk-`/`sk-proj-`, OpenRouter `sk-or-`. Passt es nicht → Hinweis ("sieht
  nicht wie ein Anthropic-Key aus, erwartet `sk-ant-…`").
- **KRITISCH — Warnung, KEINE Blockade**: Provider ändern Key-Formate; eine
  zu strenge Prüfung lehnt irgendwann *gültige* Keys ab. Also weicher Hinweis
  ("trotzdem speichern?"-Stil), **kein** hartes Verweigern. Der Nutzer kann
  immer speichern.
- **Für generische OpenAI-kompatible Provider**: Format-Prüfung **komplett
  aus** (kein vorhersagbares Format — sonst blockiert man legitime
  selbstgehostete Endpunkte).
- Rein Frontend, offline, kein Request. Ergänzt den Trim aus 0049 (der bleibt
  die eigentliche CRLF-Lösung; der Format-Hinweis ist zusätzliches Netz).

## Teil 3: "Testen"-Button für Provider-Keys (Stufe 2)

Analog zum "Verbindung testen" bei Servern (Spec 0008): ein Knopf im
Provider-Formular, der mit den **gerade eingegebenen, noch nicht
gespeicherten** Formulardaten einen minimalen Test-Request an den Provider
schickt und das Ergebnis inline zeigt.

- Nutzt den `/models`-Endpunkt (Spec 0025) bzw. einen minimalen Aufruf.
- Klares Ergebnis: **gültig** / **Authentifizierung fehlgeschlagen** /
  **nicht erreichbar** (die drei Fälle unterscheiden, nicht nur ja/nein).
- Nutzt die gerade eingegebenen Daten (wie `test_connection`), nicht die
  gespeicherten — man will *vor* dem Speichern wissen, ob der Key geht.
- Der Testen-Request unterliegt derselben Redaction/Sicherheit wie reguläre
  Provider-Requests (kein Key im Log).

## Sicherheits-/Konsistenz-Invarianten

- Format-Prüfung blockiert nie das Speichern (nur Warnung).
- Kein API-Key/Secret im Log — auch nicht beim Testen-Button.
- Die registrierten Settings-Sektionen (v. a. die Pro-Lizenz-Sektion) müssen
  nach dem Umbau **weiterhin erscheinen** — Regressionsgefahr, siehe 1.2.

## Testbarkeit

- Settings-Navigation: eine registrierte Sektion erscheint als Nav-Eintrag
  und rendert ihren Inhalt (Komponententest mit einer Test-Registrierung).
- Format-Hinweis: falscher Präfix → Warnung, Speichern trotzdem möglich;
  generischer Provider → keine Warnung.
- Testen-Button: gültiger Key → "gültig", falscher → "Auth fehlgeschlagen",
  unerreichbar → "nicht erreichbar" (gegen einen Mock-Provider).

## Reihenfolge

1. Settings-Struktur (Teil 1) — das Gerüst, inkl. registrierte Sektionen.
2. Format-Hinweis (Teil 2) — im neu strukturierten Provider-Formular.
3. Testen-Button (Teil 3) — ebenda.
So landen die neuen Provider-Features gleich an ihrem endgültigen Platz.
