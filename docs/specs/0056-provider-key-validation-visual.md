# Spec: Provider-Key-Validierung & visuelle Politur (Settings)

Status: Umgesetzt
Repo: **öffentlich** `smart_ssh`, Frontend (Provider-Formular, Settings)
Abhängigkeiten: KI-Provider (0006/0007/0025), Settings-Neustruktur (0050),
CRLF-Trim (0049), frontend-design-Skill

> Drei zusammengehörige Verbesserungen im KI-Provider-/Settings-Bereich,
> gebündelt weil sie denselben Code betreffen. **Priorität NORMAL** — reines
> Frontend (der Testen-Button braucht ggf. einen Backend-Command),
> kein server-verändernder Zugriff, keine Filter-Engine-Berührung.

## Teil 1: Format-Hinweis für API-Keys (Warnung, KEINE Blockade)

Beim Eintragen eines KI-Provider-API-Keys ein **sofortiger, offline
Format-Hinweis**, damit man nicht erst beim ersten Chat merkt, dass der
falsche Key/Provider gewählt ist.

- **Präfix-Prüfung** gegen den gewählten Provider:
  - Anthropic → `sk-ant-`
  - OpenAI → `sk-` bzw. `sk-proj-`
  - OpenRouter → `sk-or-`
- Passt das Präfix nicht → **Hinweis** („Das sieht nicht wie ein
  Anthropic-Key aus — erwartet `sk-ant-…`. Trotzdem speichern?").
- **KRITISCH — Warnung, NICHT Blockade:** Provider ändern ihre Key-Formate
  (OpenAI `sk-` → `sk-proj-`, Legacy-/Service-Account-Keys). Eine zu strenge
  Prüfung würde irgendwann *gültige* Keys ablehnen → schlimmer als keine
  Prüfung. Der Nutzer kann **immer** speichern; der Hinweis ist ein sanfter
  Stups, kein Gate.
- **Generische OpenAI-kompatible Provider**: Format-Prüfung **komplett aus**
  (kein vorhersagbares Format — sonst blockiert man legitime selbstgehostete
  Endpunkte).
- Rein Frontend, offline, kein Request. Ergänzt den CRLF-Trim (0049) — der
  bleibt die eigentliche Whitespace-Lösung, der Format-Hinweis ist
  zusätzliches Netz.

## Teil 2: „Testen"-Button für Provider-Keys

Analog zum „Verbindung testen" bei Servern (0008): ein Knopf im
Provider-Formular, der mit den **gerade eingegebenen, noch nicht
gespeicherten** Daten einen minimalen Test-Request schickt und das Ergebnis
inline zeigt.

- Nutzt einen minimalen Aufruf (z. B. `/models` aus 0025 bzw. den
  Konstruktionsweg eines echten Chat-Requests — je nach Provider).
- **Drei unterscheidbare Ergebnisse**: **gültig** / **Authentifizierung
  fehlgeschlagen** / **nicht erreichbar** (nicht nur ja/nein).
- Nutzt die **gerade eingegebenen** Daten (wie `test_connection`), nicht die
  gespeicherten — man will *vor* dem Speichern wissen, ob der Key geht.
- **Sicherheit**: Wie bei `discover_models` (0025) — ohne ausgefüllte
  Base-URL darf ein Gateway-Token **nicht** an `api.openai.com` gehen
  (derselbe Guard wie beim bestehenden Testen-Button aus 0050, falls es den
  schon gibt — prüfen, ob dieser Button bereits existiert und nur visuell/
  funktional zu ergänzen ist, statt doppelt zu bauen). Kein Key im Log
  (Redaction).

> **Hinweis:** Prüfe, ob der „Testen"-Button aus Spec 0050 bereits gebaut
> wurde. Falls ja: nicht doppelt bauen, nur in die visuelle Überarbeitung
> (Teil 3) einbeziehen und ggf. die drei-Ergebnis-Unterscheidung
> sicherstellen. Falls nein: hier bauen. **Melde mir den Ist-Stand.**

## Teil 3: Visuelle Überarbeitung (Provider-Formular + Options-Bereich)

Der Provider-/Options-Bereich wirkt aktuell **kontrastlos** — Felder,
Gruppen und Aktionen heben sich zu wenig voneinander ab, Rahmen/Struktur
fehlen.

**Stoßrichtung (Stefan):** mehr Kontrast, klarere Rahmen/Gruppierung,
Layout aufräumen. **Nutze den frontend-design-Skill** (Design-Tokens,
Kontrast-/Abstands-Regeln) — nicht Ad-hoc-Styles erfinden.

Konkret angehen:
- **Formularfelder** klarer abgrenzen (Rahmen/Hintergrund, sichtbarer
  Fokus-Zustand, Label-Feld-Zuordnung eindeutig).
- **Gruppierung** verwandter Felder (z. B. Provider-Typ / Key / Base-URL /
  Modell) visuell zusammenfassen (Card/Sektion mit Rahmen).
- **Aktionen** (Speichern, Testen, Löschen) klar als solche erkennbar,
  hierarchisch (primär/sekundär).
- **Konsistenz** mit der neuen zweispaltigen Settings-Struktur (0050) — nicht
  ein Fremdkörper-Stil, sondern dieselbe visuelle Sprache.
- Der **Format-Hinweis** (Teil 1) und das **Testen-Ergebnis** (Teil 2) sollen
  sich sauber ins neue Layout einfügen (Inline-Hinweis-Stil, nicht
  aufpoppende Fremd-Elemente).

**Wichtig:** Design ist am Ende visuell zu beurteilen (Stefan am echten
Fenster). Der Coder soll **beschreiben, was er geändert hat** (welche
Elemente Rahmen/Kontrast/Gruppierung bekamen), damit Stefan gezielt
gegenschauen kann. Keine funktionale Änderung an der Provider-Logik — nur
Darstellung.

## Nicht Teil dieser Spec
- Managed AI / Provider-Backend-Änderungen.
- Die generelle Settings-Struktur (0050, schon gebaut).

## Testbarkeit
- Teil 1: falscher Präfix → Hinweis, Speichern trotzdem möglich; generischer
  Provider → kein Hinweis; Key mit richtigem Präfix → kein Hinweis.
- Teil 2: gültiger Key → „gültig"; falscher → „Auth fehlgeschlagen";
  unerreichbar → „nicht erreichbar"; ohne Base-URL kein Token an
  api.openai.com (Guard).
- Teil 3: schwer automatisiert (visuell) — Komponententests für die Präsenz
  der neuen Struktur-Elemente; visuelle Abnahme durch Stefan.

## Reihenfolge
1. Ist-Stand Testen-Button prüfen (existiert er aus 0050?) — melden.
2. Format-Hinweis (Teil 1).
3. Testen-Button (Teil 2) — bauen oder ergänzen je nach Ist-Stand.
4. Visuelle Überarbeitung (Teil 3) — bezieht Teil 1+2 mit ein.
