# Spec 0102 — Startverzeichnis je Server für Terminal und Dateibrowser

Status: umgesetzt · Issue: #9
Zweck: Ein Server-Profil kann ein optionales Startverzeichnis tragen. Ist es
gesetzt, starten das interaktive Terminal und der SFTP-Dateibrowser einer
Sitzung dort statt im Home des Login-Nutzers. Ist es leer, bleibt alles wie
bisher. KI-Kommandos laufen weiterhin immer im Home.
Review-Priorität: NORMAL (Filter-Engine, Risiko-Klassifizierer,
Redaction, Credentials und KI-Ausführungspfad bleiben unberührt)

## 1. Ist-Stand (vor dieser Spec)

- **Terminal:** `RusshTransport::open_shell`
  (`crates/ssh-transport/src/transport.rs`) fordert PTY und Login-Shell an,
  danach läuft nichts. Der lokale Pseudo-Server startet `$SHELL` ohne cwd
  (`crates/ssh-transport/src/local.rs`).
- **Dateibrowser:** startet bei `"."` (`FileBrowserPanel.tsx`); der
  SFTP-Server löst `"."` gegen das Home auf. `remotePath.ts` zeigt `"."` als
  `~` und hält Kindpfade relativ (`./unterordner`). `SftpSession` hat kein
  `realpath`.
- **KI-Kommandos:** jede Aktion läuft in einer frischen Shell im Home
  (ADR 0059; die Secret-Lese-Prüfung im Risiko-Klassifizierer baut darauf).
- **Datenmodell:** `Server` (`crates/core/src/profiles/types.rs`); das
  jüngste vergleichbare optionale Feld ist `sftp_server_path` (Spec 0067,
  Migration `0014`).

## 2. Datenmodell

- `Server.start_directory: Option<String>`, `None` = nicht gesetzt.
- Migration `0017_server_start_directory.sql`:
  `ALTER TABLE servers ADD COLUMN start_directory TEXT;` — nullable,
  Bestandszeilen erhalten `NULL` und verhalten sich wie bisher.
- `ServerDto.start_directory` und `ServerInput.start_directory`
  (`#[serde(default)]`, fehlend = nicht gesetzt).

## 3. Validierung

Gemeinsame Regel in `ssh_manager_core::profiles::normalize_start_directory`
(Backend, beim Anlegen und Bearbeiten über
`app_logic::dto::normalize_start_directory_input`) und
`checkStartDirectory` (`frontend/src/startDirectory.ts`, im Formular vor
dem Speichern):

1. Leerraum am Rand wird entfernt; leer = nicht gesetzt.
2. Erlaubt: absoluter Pfad (`/…`) oder ein Pfad, der mit `~/` beginnt.
   Abgelehnt: jeder andere relative Pfad, auch `~` allein und `~nutzer/…`.
   Das Formular zeigt eine klare Meldung und speichert nicht.
3. Steuerzeichen werden abgelehnt (s. ADR 0101: sie würden die
   `cd`-Zeile im Terminal zerreißen oder vorzeitig abschicken).

## 4. Verhalten in der Sitzung

### 4.1 Eine Prüfung pro Sitzung

`app_logic::start_directory` prüft das konfigurierte Verzeichnis **einmal**
pro Sitzung per SFTP `stat` über den normalen SFTP-Kanal der Sitzung
(Ergebnis in einem `OnceCell` an `Session`). Terminal und Dateibrowser
teilen dieses Ergebnis:

- **gefunden** — `stat` erfolgreich und Verzeichnis;
- **fehlt** — `stat` scheitert, das Ziel ist kein Verzeichnis, oder der
  SFTP-Kanal lässt sich nicht öffnen;
- **nicht gesetzt** — keine Prüfung, kein SFTP-Zugriff.

Für `~/x` wird `./x` geprüft (SFTP kennt kein `~`, löst relative Pfade aber
gegen das Home auf), `~/` allein ist `"."`.

### 4.2 Terminal

Nach dem Start der Login-Shell schreibt `open_terminal` sichtbar
`cd -- '<dir>'` plus Eingabetaste ins PTY (Issue-Entscheidung 1). Die
Shell-Anfrage selbst ist unverändert, es gibt kein
`cd … && exec $SHELL`. Quoting und Sonderfälle: ADR 0101.

### 4.3 Dateibrowser

Neues Command `sftp_start_directory(session_id)` →
`{ path, missingDirectory }`. Der Browser öffnet bei `path`; „Zum
Startverzeichnis" (⌂) kehrt dorthin zurück, „Aufwärts" funktioniert von
dort über das bestehende `parentPath` (`/srv/app` → `/srv`, `./projects` →
`.`). Scheitert schon die Abfrage, startet der Browser wie bisher bei `"."`.

### 4.4 Fehlendes Verzeichnis

Terminal (kein `cd`) und Dateibrowser (`"."`) nutzen das Home. Die Sitzung
scheitert nicht. Genau **ein** sichtbarer, nicht blockierender Hinweis pro
Sitzung nennt das konfigurierte Verzeichnis: Das Backend gibt den Wert nur
beim ersten Abruf heraus — an `open_terminal` oder `sftp_start_directory`,
je nachdem, wer zuerst fragt — und das Frontend zeigt ihn als Toast.

### 4.5 Nicht gesetzt

Kein SFTP-Zugriff, kein `cd`, Browser bei `"."` — identisch zum bisherigen
Verhalten.

## 5. Abgrenzungen

- **KI-Kommandos** (Chat und MCP) laufen weiter im Home; ADR 0059 gilt
  unverändert. Der Ausführungspfad (`orchestration`), Filter-Engine und
  Risiko-Klassifizierer lesen den Wert nicht. Test:
  `orchestration::action_exec::tests_core::test_ai_command_is_unchanged_by_start_directory`
  (das ausgeführte Kommando ist wörtlich der Vorschlag, mit und ohne
  Startverzeichnis).
- **Keine Sicherheitsgrenze:** Terminal und SFTP laufen ohnehin nicht durch
  die Filter-Engine; das Startverzeichnis ist Komfort.
- **Lokaler Pseudo-Server:** Das Formular bietet das Feld nicht an (wie die
  übrigen in Spec 0032 ausgeblendeten Felder), das synthetische Profil trägt
  immer `None`, das Verhalten bleibt unverändert. Dass sein Dateibrowser
  früher das Arbeitsverzeichnis der App statt des Homes öffnete, ist ein
  eigener Defekt (inzwischen separat behoben) und nicht Teil dieser Spec.
- **Erhöhter Dateibrowser-Modus (Spec 0067):** Das Startverzeichnis gilt
  für den Login-Nutzer; „Zum Startverzeichnis" lädt denselben Pfad auch im
  erhöhten Modus, eine eigene Prüfung für den Zielnutzer gibt es nicht.

## 6. SSH-Config-Import/-Export (Spec 0075)

Das Feld hat in `ssh_config` kein Gegenstück und wird nicht abgebildet.
Der Export benennt es nach Spec 0075, §3.2.3, als Kommentar über dem Block
(`# smart-ssh: Startverzeichnis (…) ist hier nicht abgebildet.`); der Import
liest es nie und legt Server immer ohne Startverzeichnis an.

## 7. Tests

- Core: Validierung, SFTP-Pfad, `cd`-Zeile inkl. Leerzeichen, `'`,
  `~/`, Shell-Metazeichen (`profiles::start_directory::tests`); Export-
  Kommentar inkl. Zeilenumbruch-Ausbruch (`ssh_config::export::tests`).
- Persistenz: Rundlauf, nullable Spalte, Bestandszeile nach Upgrade.
- app-logic: Normalisierung beim Anlegen/Bearbeiten, eine Prüfung pro
  Sitzung, ein Hinweis, `cd` im PTY, Fallback bei fehlendem SFTP, KI-
  Kommando unverändert.
- Frontend: Formularvalidierung, Feld nicht beim lokalen Server,
  Dateibrowser-Start/„Zum Startverzeichnis"/„Aufwärts", Hinweis.
