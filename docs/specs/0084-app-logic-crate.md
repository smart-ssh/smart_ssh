# Spec 0084 — Tauri-freie Anwendungslogik und Schutz des erhöhten Dateikanals

Status: umgesetzt
Zweck: Die Anwendungslogik liegt in einem Crate, das Tauri nachweislich nicht kennt; die Anwendungs-Hülle enthält nur die Tauri-Anbindung. Dabei gilt für den erhöhten Dateikanal (Spec 0067 A) eine strengere, sitzungsgenaue Lebensdauer.
Bezüge: Spec 0001 (Schichtung), Spec 0067 (erhöhter Dateizugriff), Spec 0085 (Dateizugriff mit Rechteerhöhung), Spec 0090 (CI), `docs/architecture.md`, ADR 0077.

## 1. Ausgangspunkt

Die Anwendung besteht aus zwei Crates: der Tauri-freien Anwendungslogik
(Chat-Runde, Kompaktierung, Bestätigung, Verbindungstest, Verwaltung,
DTOs, gemeinsamer App-Zustand) und der Hülle, die Tauri-Commands und
-Events darauf abbildet. Die Hülle hängt von der Logik ab, nie umgekehrt.
Das ist keine Konvention, sondern wird vom Compiler und von der CI
durchgesetzt. Nach außen ändert sich nichts: dieselben Befehle, dieselbe
Schnittstelle der Hülle (Start, Verdrahtung, Edition).

## 2. Ziel

Eine Änderung an der Anwendungslogik kann Tauri nicht versehentlich
einbinden, und die KI- und MCP-Logik hat keinen Weg zum erhöhten
Dateikanal.

## 3. Anforderungen

**A1 — Der erhöhte Kanal gehört der Hülle.** Die Sitzung selbst enthält
keinen erhöhten Dateikanal. Kanal, Zugangsnachweis, Zuordnung Sitzung →
Kanal und Aktivieren/Deaktivieren liegen in der Hülle; die Zuordnung ist
eigener Zustand der Hülle. Nur die Browser-Befehle erreichen den Kanal. Die
Anwendungslogik enthält keinen Typ, keine Funktion und kein Feld, über das
er erreichbar ist.

**A2 — Kein erhöhter Kanal überlebt seine Sitzung.**
1. Das Entfernen einer Sitzung und ihres Kanals geschieht an **einer**
   Stelle der Hülle; außer ihr und Tests entfernt nichts Sitzungen. Sie
   wartet nicht auf den Transport der Sitzung. Die Sitzung wird vor dem
   Löschen des Zuordnungs-Eintrags oder unter derselben Sperre entfernt.
   Diese Stelle ist Teil des **einen öffentlichen Schließwegs** (Issue
   #816), der eine Sitzung vollständig schließt: erhöhter Kanal
   widerrufen, lokale Pfadfreigaben entfernt, wartende Bestätigung
   abgelehnt, laufender KI-Befehl abgebrochen und abgewartet (höchstens
   2 s), Transport getrennt (höchstens 2 s), Statusereignis „getrennt"
   gesendet, Editier-Kopien entfernt. Trennen, Backup-Import und jede
   einbettende App-Crate bzw. Erweiterung schließen Sitzungen nur darüber;
   es gibt keinen zweiten Weg, der nur einen Teil davon tut. Das Trennen
   hängt danach nur noch eigene Arbeit an (Chat-Sitzung beenden,
   Notiz-Vorschlag).
2. Aktivieren trägt den Kanal nur ein, wenn die Sitzung zu diesem Zeitpunkt
   noch existiert; Prüfung und Eintrag geschehen unter derselben Sperre, unter
   der auch das Entfernen (A2.1) den Eintrag löscht. Sonst endet der Befehl
   mit dem Fehler „Session nicht gefunden", und der geöffnete Kanal wird
   verworfen.
3. Ein Kanal ist nur für die Sitzung erreichbar, für die er aktiviert
   wurde, nicht für eine andere Sitzung zum selben Server.
4. Bei Verbindungsabbruch (Sitzung bleibt als getrennt stehen) verhält sich
   der Kanal wie vor dieser Spec.

**A3 — Grenze zu Tauri.** Im gesamten Abhängigkeitsbaum der Anwendungslogik
(normale, Build- und Dev-Abhängigkeiten, alle Features) gibt es kein Tauri-
oder Fenster-Paket und keinen nativen Dateidialog. Ein CI-Schritt scheitert,
wenn das verletzt ist.

**A4 — Umfang.** Jedes Modul, das in der Hülle bleibt, braucht Tauri selbst
oder ist in `docs/architecture.md` mit Grund als Ausnahme genannt.

**A5 — Testhilfen.** Testhilfen, die Tests beider Crates brauchen, liegen
in der Anwendungslogik hinter dem Feature `test-support`. Die Hülle
aktiviert es nur für ihre Tests; kein Produktivbau aktiviert es.

**A6 — Nichts ändert sich nach außen.**
1. Öffentliche Schnittstelle der Hülle: Start, Verdrahtung und Edition,
   sonst nichts.
2. Die registrierten Tauri-Befehlsnamen bleiben.
3. Kein bestehender Test entfällt oder wird ignoriert.
4. Was die Hülle aus der Logik braucht, ist dort öffentlich; sonst keine
   Sichtbarkeitserweiterung über das Crate hinaus.

**A7 — Dokumentation.** `docs/architecture.md` beschreibt die Crates und
ihre Abhängigkeitsrichtung, die Regel „was gehört wohin", den CI-Schritt
aus A3, das Feature `test-support` und warum der erhöhte Kanal in der Hülle
liegt. Der Architekturbaum in der Projektanleitung nennt die
Anwendungslogik; die verworfenen Wege stehen in einem ADR.

## 4. Abgrenzung (Schnitte)

Was Tauri braucht, bleibt in der Hülle; was Tauri-frei ist, liegt in der
Logik. Wo ein Modul beides mischte, gilt:

- Das Ausgeben von Ereignissen ist eine Schnittstelle der Logik; die
  Umsetzung für Tauri liegt als Hüllen-Typ in der Hülle.
- Reine Daten und Hilfen, die Tauri-freie Module brauchen (wartender
  Import, Erkennung des lokalen Pseudo-Servers, die Verbindungs-Zeitgrenze),
  liegen dort, wo ihre Tauri-freien Nutzer liegen.
- Bei der Zweitmeinung (Spec 0026, Spec 0039) liegt die Abruf-Logik in der
  Logik; das Lesen der Einstellungen bleibt in der Hülle und wird als Wert
  übergeben.
- Eine eigene Sperre je Sitzung schützt den erhöhten Kanal; die Sperre der
  Zuordnung gilt nur für Nachschlagen, Eintragen und Entfernen, damit ein
  laufender Vorgang in einer Sitzung das Trennen einer anderen nicht
  aufhält.

## 5. Sicherheitszusagen

- Der erhöhte Kanal (Spec 0067 A) ist strenger geschützt als zuvor: KI- und
  MCP-Logik liegen in einem Crate, das den Kanal nicht kennt. Daraus
  ergeben sich die Risiken aus A2 (Kanal überlebt die Sitzung, gehört zur
  falschen Sitzung, Wettlauf mit dem Trennen), die T5–T10c abdecken.
- Filter-Engine, Risiko-Klassifizierung, Schwärzung, Bestätigung und
  Credentials sind durch die Aufteilung nicht verändert.

## 6. Nachweis (Testfälle)

- **T1** Die Menge der Tests blieb erhalten; neu sind nur T5b–T10c und die
  Widerrufs-Tests aus §9.
- **T3 — Grenze wirkt.** Der CI-Schritt aus A3 scheitert nachweislich, wenn
  eine Tauri-Abhängigkeit eingetragen wird.
- **T4 — Umfang und Feature.** Die in der Hülle verbliebenen Module
  erfüllen A4; `test-support` ist in keinem Produktivbau aktiv. Der
  CI-Schritt fragt den Baum der Logik ab, nicht den der Workspace-Wurzel.
- **T5** KI-Dateiaktionen und MCP-Dateiaktionen laufen nie über den
  erhöhten Kanal. **T5b** dasselbe für eine MCP-**Schreib**aktion.
- **T6** Kanal aktiv, Sitzung wird über die Stelle aus A2.1 entfernt: Die
  Zuordnung enthält danach keinen Eintrag für diese Sitzung.
- **T7** Zwei gleichzeitige Sitzungen zum selben Server, Kanal nur in A
  aktiv: Browser-Befehle von B laufen über den normalen Kanal, Deaktivieren
  in B lässt A aktiv.
- **T8** Wettlauf: Aktivieren ist nach den Proben, vor dem Eintrag
  angehalten; die Sitzung wird entfernt; danach läuft das Aktivieren weiter.
  Ergebnis: „Session nicht gefunden", kein Eintrag in der Zuordnung.
  **T8b:** dasselbe, während das Entfernen zwischen Sitzung und Eintrag
  steht.
- **T9** Deaktivieren bei nicht aktivem Kanal und für eine unbekannte
  Sitzung: kein Absturz; „nicht aktiv" für eine bekannte Sitzung ohne
  Kanal, „Session nicht gefunden" für eine unbekannte.
- **T10** Kanal von einem Befehl festgehalten, Deaktivieren, danach
  scheitert der Zugriff. **T10b** dasselbe mit Neu-Aktivieren für einen
  anderen Nutzer. **T10c** dasselbe mit Entfernen der Sitzung.

## 7. Grenzen

- Dass KI und MCP den Kanal nicht erreichen, tragen Signaturen und
  Crate-Grenze; T5/T5b scheitern nur, wenn ein solcher Weg entsteht.
- Die Zusage, dass nur eine Stelle Sitzungen entfernt (A2.1), sichert ein
  Wächtertest, der die Quelltexte nach Aufrufen durchsucht (Selbsttest mit
  eingepflanztem Aufruf), nicht der Compiler.

## 9. Klarstellungen

(Abschnitt 8 entfällt; die Nummerierung bleibt, weil Code und Tests auf
Abschnitt 9 verweisen.)

- **Der Widerruf wirkt beim Zugriff, nicht beim Befehlsbeginn.** Ein
  Browser-Befehl darf einen erhöhten Kanal nicht über das Deaktivieren oder
  Entfernen hinaus benutzen. Der Kanal wird im Moment der Nutzung unter
  seiner Sperre geprüft; ist er inzwischen deaktiviert, entfernt oder für
  einen anderen Nutzer neu aktiviert, endet der Befehl mit
  `ELEVATED_CHANNEL_INACTIVE` bzw. dem bisherigen Nutzer-Fehler. Deaktivieren
  wartet, bis ein laufender Vorgang auf dem Kanal fertig ist.
- **Der Widerruf wirkt sofort.** Der Kanal trägt einen Widerrufs-Merker, der
  ohne Sperre sofort gesetzt und bei **jedem** Zugriff geprüft wird. Das
  Entfernen einer Sitzung (A2.1) setzt nur diesen Merker und wartet auf
  nichts; so liefe ein bereits wartender Browser-Befehl nicht noch einmal
  mit den alten Rechten, und das Trennen bliebe nicht hinter einem
  hängenden Transfer stehen. Ein widerrufener Kanal meldet auch keinen
  Ziel-Nutzer mehr, damit keine Protokollzeile eine Erhöhung behauptet, die
  nicht stattfand.
- Deaktivieren für eine **unbekannte** Sitzung liefert „Session nicht
  gefunden"; „nicht aktiv" gilt nur für eine bekannte Sitzung ohne Kanal.
