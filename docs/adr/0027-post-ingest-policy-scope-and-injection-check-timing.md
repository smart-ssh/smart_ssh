# 0027-post-ingest-policy-scope-and-injection-check-timing

## Status
Vorgeschlagen

## Kontext

`docs/specs/0039-untrusted-content-fencing.md`, Abschnitt 5, ersetzt die
alte SEC-03-Bremse (Spec 0013, Abschnitt 3.3: ab Runde 2 wird **jede**
`SuggestCommand`/`ReadRemoteFile`-Aktion zu `Confirm`, sobald in der
Sitzung Serverinhalt eingelesen wurde) durch die konfigurierbare
`PostIngestPolicy` (`Strict`/`Balanced`/`Standard`, Default `Balanced`).
Ein unabhängiger Review-Pass nach Abschluss der Implementierung
(spec-reviewer-Rolle, ERHÖHTE Priorität) hat zwei Stellen markiert, an
denen der Code eine Spec-Formulierung auf eine von mehreren plausiblen
Arten liest, ohne dass die Spec selbst das exakt vorschreibt — beides mit
sicherheitsrelevanter Tragweite, daher hier explizit festgehalten statt
nur im Code-Kommentar.

## Entscheidungen

**1. `Balanced` prüft ausschließlich die Server-Risiko-Achse
(`RiskAssessment::server_risk != None`), nicht die Daten-Risiko-Achse.**
Spec 0039, Abschnitt 5.1 definiert "verändernde Aktion" für `Balanced`
wörtlich über "modifying" im Sinne des Spec-0026-Risiko-Klassifizierers,
ohne die Daten-Risiko-Achse zu erwähnen. Das ist so umgesetzt
(`orchestration.rs`, `handle_action_proposed`,
`is_modifying_action = risk_assessment.server_risk != RiskLevel::None`).

**Konsequenz, die die Spec nicht explizit benennt:** eine reine
Leseaktion mit Server-Risiko `None`, aber Daten-Risiko `Red` — z. B.
`sftp-read /root/.ssh/id_rsa` oder `cat /etc/shadow` — bleibt unter
`Balanced` nach dem Einlesen von Serverinhalt weiterhin `AutoExec`, sofern
eine Allow-Regel greift. Das ist eine tatsächliche Abschwächung
gegenüber der alten SEC-03-Garantie (Spec 0013), die **jede** Aktion nach
Runde 1 eskaliert hätte, unabhängig von irgendeiner Risiko-Achse — für
genau das Szenario, das SEC-03 ursprünglich adressiert (ein durch
eingeschleuste Anweisungen zur Exfiltration bewegter Copilot), ist das
die sicherheitsrelevanteste Lücke in dieser Eskalationsstufe.

**Bewusst NICHT geändert (Verschärfung um Daten-Risiko-Achse
zurückgestellt):** Spec 0039 Abschnitt 5.1 ist an dieser Stelle
unzweideutig auf die Server-Risiko-Achse formuliert; `Balanced` ist als
Kompromiss zwischen `Strict` (jede Aktion) und `Standard` (keine
zusätzliche Eskalation) explizit so beschrieben, dass "reine
Leseaktionen weiter nach den bestehenden Regeln laufen". Eine
nachträgliche Erweiterung um die Daten-Risiko-Achse in diesem Schritt
wäre eine über den expliziten Spec-Text hinausgehende Verschärfung —
sinnvoll, aber eine eigene, vom Nutzer zu treffende Produktentscheidung
(ändert das Standardverhalten für alle Server, für die `Balanced` per
Default aktiv ist), kein stiller Fix im Rahmen dieses Reviews. Wer diese
Lücke schließen will, deaktiviert `Strict` server-spezifisch, oder eine
künftige Spec-Änderung erweitert `Balanced`s Definition explizit um
`data_risk != None`.

**2. Die KI-Prüfung auf eingeschleuste Anweisungen
(`check_for_injected_instructions`) läuft inline `await`ed, nicht als
eigener `tokio::spawn`-Task.** Spec 0039, Abschnitt 5.2 beschreibt das als
"läuft asynchron, blockiert nicht den regulären Ablauf". Wörtlich
gelesen könnte das einen vollständig vom aufrufenden Turn losgelösten
Hintergrund-Task verlangen. Umgesetzt ist stattdessen dieselbe Lesart, die
bereits für die strukturell identische Risiko-Zweitmeinung aus Spec 0026
gilt (`fetch_second_opinion`, ebenfalls inline `await`ed): "asynchron"
heißt hier "verzögert nicht die für die aktuelle Aktion bereits
gesendeten Events" (das `chat-action-proposed`-Event ist zum Zeitpunkt
der Prüfung noch nicht gesendet), nicht "detached vom gesamten
Anfrage-Antwort-Zyklus dieser Runde".

**Begründung für diese Lesart statt eines echten `tokio::spawn`:** ein
detached Task bräuchte Zugriff auf `Session`/`EventEmitter` über das Ende
der aktuellen Funktion hinaus — das würde `Arc<Session>` und
`Arc<dyn EventEmitter>` durch die gesamte Aufruf-Kette bis zu
`execute_suggested_command`/`execute_read_remote_file` durchreichen
müssen, ein unverhältnismäßiger struktureller Umbau für einen einzelnen
zusätzlichen `.await` auf eine bereits kurze, weil bewusst
kontextarme Anfrage (s. `fetch_injection_check`s Doc-Kommentar:
"minimaler Kontext"). Da genau dieselbe Abwägung für Spec 0026 bereits
getroffen wurde und dort niemand die dortige Wortwahl als Verstoß
gewertet hat, überträgt diese Entscheidung dieselbe Lesart auf Spec 0039.

**Konsequenz:** ein Aktionsvorschlag, für den die Prüfung läuft, braucht
sichtbar länger (ein zusätzlicher Provider-Roundtrip), bevor
`chat-action-proposed` gesendet wird — sichtbar als kurze zusätzliche
Wartezeit in der UI, nicht als Fehlverhalten, aber ein Unterschied zu
einer strikt gelesenen "läuft nebenher"-Interpretation.

## Konsequenzen

**Positiv:**
- `Balanced`s Definition bleibt exakt spec-konform (Server-Risiko-Achse),
  keine über den Spec-Text hinausgehende, unautorisierte Verschärfung des
  Standardverhaltens für bestehende Installationen.
- Die Injection-Prüfung teilt sich Infrastruktur und Architektur-Präzedenz
  vollständig mit der bereits etablierten Risiko-Zweitmeinung (Spec 0026)
  — keine zweite, abweichende Nebenläufigkeits-Lösung im selben Modul.

**Negativ / Trade-off:**
- Unter dem Default `Balanced` bleibt ein gezielter Exfiltrations-Lesezugriff
  (Daten-Risiko `Red`, Server-Risiko `None`) nach dem Einlesen von
  Serverinhalt ohne zusätzliche Bestätigung, wenn eine Allow-Regel greift
  — eine tatsächliche Abschwächung gegenüber der alten SEC-03-Garantie.
  Nutzer, die dieses Szenario abgedeckt haben wollen, müssen `Strict`
  wählen. Empfehlung für eine künftige Spec-Iteration: `Balanced` explizit
  um die Daten-Risiko-Achse erweitern.
- Die Injection-Prüfung verzögert messbar den Zeitpunkt, zu dem eine
  Aktion dem Nutzer vorgeschlagen wird, wenn `ai_injection_check_enabled`
  aktiv ist — kein "unblockierter Hintergrund-Task" im wörtlichen Sinn.
