# Spec: Sicherheitshärtung vor dem Launch

Status: Entwurf
Repo: **öffentlich** `smart_ssh` — `crates/core` (Redactor, Risiko-
Klassifizierer), `crates/app-shell` (Orchestrierung, Confirm, Host-Key-
Registry), `crates/ai-providers` (discovery.rs), Frontend (Schreib-Dialog)
Abhängigkeiten: Redactor + Härtungen (Shadow/DB/Token-Runden), Risiko-
Klassifizierer (0022/0026), Filter-Engine + Glob-Fix (0002/0060),
Untrusted-Eskalation (0039), Datei-Write + Sudo-Fallback (0020),
max_tokens (0065), Stopp/Einreihen (0066), Body-Timeout-Fix,
Confirm-Timeout (0046 Fund 4)

> Fünf kleine Punkte, die ein sicherheitsbewusster Nutzer beim ersten
> Ausprobieren finden könnte. Bei einem Sicherheitsprodukt kostet so ein Fund
> nach dem Launch mehr als vorher.
>
> **Grundsatz für alle fünf Teile: Jede Änderung verschärft, keine lockert.**
> Bei jedem Teil muss gelten: Kein bestehender Schutz wird aufgeweicht, keine
> bestehende Regel/kein bestehendes Muster wird schwächer.
> **Priorität ERHÖHT, Review adversarial.**

---

## Teil 1 — Redaction: nackte API-Keys und Auth-Header

**Problem:** Die Redaction erkennt nackte Key-Formate nicht, wenn kein
Schlüsselwort (`password=`, `token=`) davorsteht. Genau diese Keys liegen bei
unserer Zielgruppe in `.env`-Dateien, Configs und Logs auf ihren Servern —
inklusive der Keys der Provider, die die App selbst unterstützt.

**Ergänzen** (jeweils das exakte, eindeutige Format — **gegen die aktuelle
Provider-Doku bzw. echte Beispielformate prüfen, nicht aus dem Gedächtnis**):
- Anthropic (`sk-ant-…`)
- OpenAI (`sk-proj-…`, Service-Account-/Admin-Formate)
- OpenRouter (`sk-or-…`)
- GitLab (`glpat-…`), Hugging Face (`hf_…`)
- HTTP-Header: `x-api-key: …`, `Authorization: Basic <base64>`
- Generische URL-Zugangsdaten `schema://user:pass@host` außerhalb der
  bekannten DB-Schemata (http/https/ftp/redis …) — **Achtung:** die
  DB-String-Runde hatte zwei Regressionen durch zu gierige Zeichenklassen
  (siehe Backlog); dieselbe Lehre anwenden.

**Lehren aus den früheren Redaction-Runden (verbindlich):**
- **Reihenfolge**: Neue Muster so einordnen, dass kein anderes Muster sie
  vorher zerteilt (Shadow-Fix-Lehre) — und umgekehrt kein neues Muster ein
  bestehendes zerstört (DB-String-Lehre, „never loosen an existing check").
- **Falsch-Positive testen**: Ein bloßes `sk-` in normalem Text, Git-SHAs,
  UUIDs, Base64-Bilddaten dürfen nicht getroffen werden.
- **Kein generisches Hoch-Entropie-Fallback** (analysiert und verworfen).

## Teil 2 — Filter: Lesebefehle auf Secret-Pfade eskalieren

**Problem:** Die Prompt-Regel aus 0066 („sensible Dateien nicht lesen") ist
weich — die KI folgt ihr meistens, nicht garantiert. Es braucht die harte
Ergänzung in der Risiko-Klassifizierung.

- **Bestehenden Mechanismus erweitern**, keinen neuen bauen: Es gibt bereits
  ein Risiko-Muster für Lesebefehle auf `shadow` (`data_risk: Red`). Dieses
  Muster-Set um typische Secret-Pfade erweitern.
- **Pfade** (Vorschlag, prüfen/ergänzen): `~/.ssh/id_*` (**nicht** `*.pub`),
  `*.pem`, `*.key`, `.env` / `.env.*` (Grenzfall `.env.example`: begründet
  entscheiden, im Zweifel eskalieren), `/etc/shadow`, `/etc/gshadow`,
  `~/.aws/credentials`, `~/.docker/config.json`, `~/.kube/config`,
  `~/.netrc`, `~/.pgpass`, `~/.git-credentials`.
- **Befehle**, die Inhalt in die Ausgabe bringen: `cat`, `less`, `more`,
  `head`, `tail`, `bat`, `grep`, `sed`, `awk`, `xxd`, `od`, `strings`,
  `base64`, `openssl` (Key-Ausgabe) — **und die Datei-Lese-Aktion**
  (`ReadRemoteFile` / `sftp-read`), sowohl im Chat- als auch im MCP-Pfad.
  **Nicht** eskalieren: `cp`, `install`, `mv`, Umleitungen ohne Ausgabe —
  das sind genau die Wege, die die Prompt-Regel empfiehlt.
- **Wirkung**: Treffer → **immer Confirm, nie AutoExec — auch wenn eine
  User-Allow-Regel greift.** Das ist eine reine Eskalation (analog zur
  Untrusted-Eskalation aus 0039), sie kann nichts lockern.
- **Fail-safe bei Unklarheit**: Shell-Metazeichen/Quoting im Pfad-Token
  (0060-Lehre) → im Zweifel eskalieren (strenger ist hier die sichere
  Richtung). Verkettungen (`echo x; cat ~/.ssh/id_rsa`) über die bestehenden
  Sub-Kommando-Traces.
- **Ehrlich dokumentieren**: lexikalisch, über Variablen/Symlinks umgehbar
  — Redaction und Confirm bleiben die weiteren Schichten.

## Teil 3 — Sudo-Fallback im Schreib-Bestätigungsdialog ankündigen

**Problem (Release-Gate C, MUSS):** Beim Datei-Schreiben (0020) nutzt die App
ggf. das gespeicherte Sudo-Passwort, wenn der normale Write an Rechten
scheitert. Der Bestätigungsdialog zeigt das **nicht** an — die bestehende
„nutzt gespeichertes Sudo-Passwort"-Warnung prüft nur `SuggestCommand`.

- Der Schreib-Bestätigungsdialog zeigt **vor** der Bestätigung deutlich an,
  wenn der Write mit Sudo erfolgen wird bzw. kann (gleicher Stil wie die
  bestehende Warnung).
- Wird Sudo erst *nach* der Bestätigung als Fallback nötig (Rechte-Fehler
  beim ersten Versuch): **nicht still** mit Sudo nachschieben, sondern
  entweder bereits im Dialog als Möglichkeit ankündigen oder erneut
  bestätigen lassen. Beschreibe mir, wie der Fallback heute abläuft, und
  wähle die Variante, bei der der Nutzer nie ohne Wissen Sudo auslöst.
- Gilt für Chat **und** MCP.

## Teil 4 — Multi-`tool_use`-Pfad mit Tests absichern

**Problem:** Eine Antwort mit **mehreren vollständigen** Tool-Calls wird
sequenziell verarbeitet — funktioniert heute, hat aber **keinen einzigen
Test** (Batching-Diagnose). Provider dürfen das jederzeit schicken
(`tool_choice: auto`, OpenAI parallele `tool_calls`).

Tests (Anthropic- und OpenAI-Format), mindestens:
- Zwei vollständige Tool-Calls → beide einzeln durch die Filter-Engine,
  jeweils eigene Confirm-/AutoExec-Entscheidung.
- **Eskalation greift über Aktionen hinweg**: Aktion 1 liest untrusted
  content → Aktion 2 derselben Antwort wird eskaliert (0039).
- Aktion 1 abgelehnt → Aktion 2 wird trotzdem einzeln entschieden (nicht
  still mit-abgelehnt und nicht still mit-ausgeführt) — prüfe das Ist-
  Verhalten und dokumentiere es; falls es heikel ist, melden statt raten.
- Stopp (0066) zwischen Aktion 1 und 2 → Aktion 2 wird nicht ausgeführt.
- Kombination mit `max_tokens` (0065) bleibt: letzte abgeschnitten → keine.
Findet der Test ein Fehlverhalten, **beheben** (und melden).

## Teil 5 — Timeouts: Host-Key-Bestätigung und discovery.rs

**5a — discovery.rs** (Modell-Laden, Attestierung): Verbindung und
Body-Lesen **ohne** Timeout. Denselben Mechanismus wie im Provider-Pfad
nutzen (`SSE_INACTIVITY_TIMEOUT` bzw. `read_error_body_with_timeout`) — keine
neue Konstante.

**5b — Host-Key-Bestätigung** (`pending_host_key_confirmations`): hat **gar
keinen** Timeout, kann nach Frontend-Verlust ewig warten.
- Timeout wie beim Confirm-Timeout (0046 Fund 4).
- **Timeout = Ablehnung.** Ein Host-Key wird durch Zeitablauf **nie**
  akzeptiert (Invariante 5: Host-Key-Änderungen nie automatisch akzeptiert).
- **Die bekannte Falle** (im Backlog und im Doc-Kommentar von
  `ConfirmationRegistry::cancel()` beschrieben): Ein `cancel()` auf eine
  `SessionId`, die inzwischen von einem `connect()`-Retry neu registriert
  wurde, würde den **falschen** Eintrag löschen. Lösung über eine
  **Generation/Identität pro Eintrag** — der Timeout darf nur *seinen*
  Eintrag entfernen. Regressionstest genau für diesen Fall.

---

## Invarianten
- Kein Teil lockert einen bestehenden Schutz (Muster, Regel, Eskalation).
- Secret-Pfad-Eskalation ist reine Verschärfung, übersteuert Allow-Regeln
  nur Richtung Confirm.
- Kein Sudo ohne Wissen des Nutzers.
- Host-Key nie durch Timeout akzeptiert; Timeout entfernt nur den eigenen
  Eintrag.
- Keine neuen Timeout-Konstanten, wo es schon welche gibt.

## Reihenfolge / Commits
1. Teil 1 (Redaction)
2. Teil 2 (Secret-Pfad-Eskalation)
3. Teil 3 (Sudo-Ankündigung)
4. Teil 4 (Multi-tool_use-Tests + ggf. Fixes)
5. Teil 5 (Timeouts)

## Abschluss
- `spec-reviewer` **ERHÖHT, adversarial** — je Teil: Umgehungsversuche
  (Teil 1: Muster-Kollisionen/Falsch-Negative; Teil 2: Quoting, Verkettung,
  MCP-Pfad, Allow-Regel-Übersteuerung; Teil 3: stiller Sudo-Fallback; Teil 4:
  Eskalation zwischen Aktionen; Teil 5: falscher Eintrag gelöscht, Host-Key
  durch Timeout akzeptiert).
- CHANGELOG (Security-Kategorie).
- Manuelle Testabläufe für Stefan.
