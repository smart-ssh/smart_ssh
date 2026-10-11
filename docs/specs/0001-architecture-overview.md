# Spec 0001 — Architektur-Übersicht

Status: umgesetzt
Zweck: Ziel des Projekts und die Grundregeln, nach denen alle anderen Bereiche aufgebaut sind: Schichtung, Testbarkeit der Außenanbindungen und Entwicklungsprozess.
Bezüge: Spec 0002 (Filter-Engine), Spec 0084 (Abgrenzung der beiden Anwendungs-Crates), ADR 0005, ADR 0006.

## 1. Ziel des Projekts

Ein plattformübergreifender SSH-Client (macOS, Windows, Linux) mit:

- Server-Verwaltung (Profile, Passwort-, Schlüssel- und Zertifikats-Anmeldung)
- KI-Chat: Der Nutzer chattet mit einer frei wählbaren KI (eigene
  API-Schlüssel), die SSH-Kommandos vorschlägt
- Vorschlägen, die durch eine Filter- und Regel-Engine laufen, die je nach
  Regel automatisch ausführt oder eine ausdrückliche Bestätigung verlangt
- dem Kernprinzip des gesamten Projekts: **volle Transparenz und Kontrolle
  über jedes Kommando, das auf einem Server landet.** Die KI ist Copilot,
  nie autonomer Akteur ohne Sichtbarkeit für den Nutzer.

## 2. Technik-Entscheidung

Die Anwendung ist eine Desktop-Anwendung mit einem Rust-Kern und einer
Web-Oberfläche (Tauri 2, Frontend in React). Gründe, kurz:

- geringe Paketgröße und geringer Ressourcenverbrauch, kein mitgeliefertes
  Node.js-Backend als zusätzliche Angriffsfläche
- ausgereifte SSH-Bibliothek in Rust
- Zugriff auf den Schlüsselbund des Betriebssystems ohne
  plattformspezifischen Sonderaufwand
- Die Kernlogik ist eine oberflächenunabhängige Bibliothek und lässt sich
  später unverändert in einer Terminal-Oberfläche wiederverwenden.

Verworfen wurden Electron (größere Angriffsfläche, höherer
Ressourcenverbrauch) und eine native Implementierung je Plattform
(dreifacher Wartungsaufwand, keine geteilte Kernlogik).

## 3. Schichtung

Die Anwendung besteht aus diesen Schichten (die Crate-Übersicht steht in
der Projektanleitung für Beitragende, die Abgrenzung der beiden
Anwendungs-Crates in Spec 0084):

- **Kern:** reine Fachlogik ohne Oberfläche und ohne Tauri — SSH-Verbindung
  (als Schnittstelle), Filter-Engine, Risiko-Einstufung, KI-Anbindung mit
  Ausgabe-Schwärzung, Server-, Gruppen- und Credential-Typen.
- **Konkrete Anbindungen:** je eine Crate für Transport, Persistenz,
  Schlüsselbund, KI-Anbieter und MCP-Server.
- **Anwendungslogik:** Tauri-freie Abläufe (Chat-Runde, Bestätigung,
  Verwaltung) über dem Kern.
- **Anwendungs-Hülle:** dünner Wrapper, der Tauri-Commands und -Events auf
  die Anwendungslogik abbildet.
- **Frontend:** spricht ausschließlich mit der Anwendungs-Hülle.

**Verbindliche Regel:** Der Kern hat keine Abhängigkeit auf Tauri oder ein
UI-Framework. Jede fachliche Logik gehört in den Kern oder die
Anwendungslogik, nicht in die Hülle. Das stellt sicher, dass eine spätere
Terminal-Oberfläche dieselbe Logik ohne Duplikation nutzen kann.

Eine Server-Identität (auch der lokale Pseudo-Server) wird in der
Sicherheitslogik nie unterschieden; Sonderbehandlung gehört an die Stelle,
an der der Transport gebaut wird (Spec 0032).

## 4. Testbarkeits-Prinzip

Für jede Außenanbindung (SSH-Verbindung, Dateiübertragung, interaktive
Shell, KI-Anbieter, Regel-, Profil- und Credential-Speicher) wird eine
Schnittstelle definiert, bevor die konkrete Umsetzung entsteht. Jede hat
eine In-Memory- oder Mock-Umsetzung für Tests. Dadurch lässt sich jede
Fachlogik — vor allem die sicherheitskritische Filter-Engine — ohne echte
SSH-Verbindung, ohne API-Aufruf und ohne Schlüsselbund testen. Jedes
fachliche Modul im Kern hat eine eigene Testsuite; die Oberfläche bleibt
bewusst dünn und wird nicht auf dieselbe Weise durchgetestet.

## 5. Entwicklungsprozess

- **Spec-first je Bereich:** Jeder fachliche Bereich hat eine nummerierte
  Spec (Form: `docs/specs/README.md`), die im selben Pull Request wie die
  Änderung angepasst wird.
- **Tests vor oder parallel zur Umsetzung**, besonders bei
  sicherheitskritischen Modulen.
- **Kleine, prüfbare Schritte.**
- **ADRs** für Entscheidungen, die über eine einzelne Spec hinausgehen.
- **Test-Gate vor jedem Commit:** Formatprüfung, Clippy ohne Warnungen und
  alle Tests müssen grün sein (lokal und in der CI, Spec 0090).
- **Unabhängiger Review nach jedem Umsetzungsschritt:** durch eine Instanz,
  die nicht an der Umsetzung beteiligt war, gegen die betroffene Spec bzw.
  das Issue und den Diff; sie ändert selbst keinen Code und liefert einen
  strukturierten Bericht. Bei sicherheitskritischen Modulen (Filter-Engine,
  Risiko-Klassifizierer, Schwärzung, Credential-Handling) mit erhöhter
  Priorität, inklusive gezielter Umgehungsversuche.

## 6. Grenzen

- Ein Teamfreigabe-Konzept für Server-Profile gibt es nicht.
- Die Lizenz des Projekts ist Apache-2.0.
