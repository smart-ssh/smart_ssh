# Spec: Abbruch lang laufender KI-Kommandos

Status: Entwurf
Modul: Erweiterung `crates/core/src/ssh/`, `crates/ssh-transport/`,
`crates/app-tauri`, `frontend/`
Abhängigkeiten: SSH-Transport (Spec 0005), Kernschleife (Spec 0007),
Bestätigungs-Registry (Spec 0007, `ConfirmationRegistry`), i18n (Spec 0024)

## 1. Ziel

`SshTransport::execute()` wartet aktuell bedingungslos, bis der Remote-Kanal
schließt — bei einem nicht selbst terminierenden Kommando (`journalctl -f`,
`tail -f`, `watch`, `kubectl logs -f`, …) heißt das: für immer. Da
`execute_suggested_command` währenddessen `session.transport` sperrt, hängt
nicht nur die aktuelle Chat-Runde, sondern jede weitere Interaktion mit der
Session (neues Terminal-Tab, Trennen, jedes weitere KI-Kommando).

Diese Spec ergänzt eine **manuelle** Abbruchmöglichkeit: Läuft ein
KI-vorgeschlagenes Kommando länger als 5 Sekunden, erscheint ein Indikator
an der Aktionskarte. Ein Klick schließt **ausschließlich den Exec-Kanal
dieses einen Kommandos** (nicht die SSH-Verbindung, nicht die Session,
nicht andere offene Kanäle wie Terminal/SFTP) und liefert die bis dahin
gesammelte Ausgabe als Ergebnis zurück in den Chat-Kontext — derselbe Pfad,
über den auch ein regulär beendetes Kommando sein Ergebnis meldet.

**Nicht Teil dieser Spec:**
- Kein automatischer Timeout. Der Abbruch ist eine bewusste Nutzeraktion,
  keine geratene feste Zeitgrenze, die ein legitim lang laufendes Kommando
  (z. B. `apt upgrade`) fälschlich beenden würde.
- Kein "echtes" Live-Mitlesen für die KI. Die KI sieht ohnehin immer nur das
  Ergebnis eines bereits abgeschlossenen Tool-Aufrufs, nie einen
  fortlaufenden Strom zwischen zwei Chat-Runden — "der KI beim Log-Folgen
  zusehen lassen" hat im aktuellen Anfrage/Antwort-Modell kein sinnvolles
  Gegenstück. Was tatsächlich gebraucht wird, ist ein *begrenzter*
  Ausschnitt, kein unbegrenzter Strom — genau das liefert der Abbruch.
- Kein garantiertes Töten des Remote-Prozesses (s. Abschnitt 3, letzter
  Absatz).

## 2. Frontend: Lauf-Indikator nach 5 Sekunden

Rein clientseitig, **kein neues Backend-Event** für den Start nötig: Sobald
eine Aktionskarte in den Ausführungszustand übergeht — sofort bei
`AutoExec`, oder nach Klick auf "Ausführen" im Bestätigungsdialog — startet
ein lokaler Timer. Liegt nach 5 Sekunden noch kein `chat-action-result` für
diese `actionId` vor, erscheint ein dezenter Indikator ("läuft seit {{s}}s…")
mit einem Button. Nur für `SuggestCommand`-Aktionen relevant —
`ReadRemoteFile`/`WriteRemoteFile` laufen über die SFTP-Session (Spec 0020),
nicht über `execute()`, und sind von diesem Hänge-Muster nicht betroffen
(s. Abschnitt 5).

Button-Text bewusst nicht "Kommando stoppen" (suggeriert einen garantierten
Kill) — stattdessen ehrlich formuliert im Sinne von "Verbindung zu diesem
Kommando trennen". Klick ruft `cancel_running_command(sessionId, actionId)`
auf, der Button wechselt in einen deaktivierten Zwischenzustand ("wird
getrennt…"), bis das reguläre `chat-action-result`-Event eintrifft — exakt
derselbe Pfad wie bei reguläre Beendigung, kein Sonderfall im Frontend
nötig außer dem Indikator selbst und einem Hinweis in der Ergebnisanzeige
(s. Abschnitt 4).

## 3. Backend: Abbrechbare Ausführung

Neue Methode auf `SshTransport` (Spec 0005), mit Default-Implementierung,
die Cancel ignoriert und unverändert `execute()` ruft — bestehende
Implementierungen/Mocks bleiben ohne Änderung funktionsfähig:

```rust
pub struct ExecOutcome {
    pub output: CommandOutput,
    /// `true`, wenn der Abbruch tatsächlich gegriffen hat, bevor das
    /// Kommando von selbst beendet war.
    pub cancelled: bool,
}

#[async_trait]
pub trait SshTransport: Send + Sync {
    // ... bestehende Methoden unverändert ...

    async fn execute_cancellable(
        &mut self,
        command: &str,
        cancel: oneshot::Receiver<()>,
    ) -> Result<ExecOutcome, SshError> {
        Ok(ExecOutcome { output: self.execute(command).await?, cancelled: false })
    }
}
```

**Reale Implementierung** (`ssh-transport`): `drain_channel` liest nicht
mehr bedingungslos bis der Kanal schließt, sondern in einem `tokio::select!`
zwischen dem nächsten `channel.wait()` und dem Cancel-`Receiver`. Löst
Cancel zuerst aus, schließt die Funktion den Kanal aktiv (`channel.eof()`
bzw. Drop) und liefert `accumulate_exec_output(messages)` mit der bis dahin
gesammelten Ausgabe zurück, `cancelled: true`. `exit_code` bleibt `None` —
bereits ein an anderer Stelle behandelter gültiger Zustand für "kein
regulärer Exit", kein neuer Sonderfall im Typ nötig.

**Cancel-Registrierung**: `AppState` bekommt eine neue Registry,
wiederverwendet exakt den bestehenden generischen Typ
(`crate::confirmation::ConfirmationRegistry`, bisher für
Host-Key-Bestätigung/Aktions-Freigabe genutzt):

```rust
pub running_command_cancellations: ConfirmationRegistry<ActionId, ()>,
```

`execute_suggested_command` registriert vor dem Aufruf einen `Receiver`
unter der `action_id`, ruft `execute_cancellable` statt `execute()`, und
entfernt den Eintrag implizit (die Registry räumt bei `resolve()`/Verbrauch
selbst auf). Neuer Tauri-Command:

```rust
#[tauri::command]
pub async fn cancel_running_command(
    state: State<'_, AppState>,
    action_id: ActionId,
) -> CommandResult<()>
```

löst den passenden Sender aus, falls einer wartet. Kein Fehler, falls
nicht (Race zwischen Klick und regulärer Beendigung — das Kommando ist dann
bereits fertig, der Klick kommt schlicht zu spät und wird stillschweigend
ignoriert, kein Absturz, keine Fehlermeldung an den Nutzer für einen
harmlosen zeitlichen Zufall).

**Kontext für die KI**: `MessageContent::CommandResult` und
`ActionResultPayload::Command` bekommen je ein zusätzliches
`cancelled: bool`-Feld. Die Kontext-Formatierung für den KI-Request (beide
Provider-Implementierungen, `ai-providers`) ergänzt bei `cancelled: true`
einen expliziten Hinweis im selben Block wie den bestehenden
`security_notice` — die KI muss erkennen können, dass die Ausgabe
unvollständig ist und das Fehlen eines Exit-Codes keine Störung, sondern
ein manueller Abbruch war, sonst könnte sie den fehlenden Exit-Code
fälschlich als Kommandofehler interpretieren und z. B. denselben Befehl
erneut vorschlagen.

**Realität des Abbruchs**: Ein Schließen des Exec-Kanals beendet
zuverlässig **nur das lokale Warten** — es ist **kein** garantiertes Töten
des Remote-Prozesses. Anders als beim Terminal-Tab (echtes PTY, `Ctrl+C`
erreicht die Prozessgruppe direkt, s. Spec 0005/0017) hat ein reiner
Exec-Kanal keine kontrollierende TTY. Die meisten CLI-Werkzeuge
(einschließlich `journalctl`) beenden sich in der Praxis aber zeitnah
selbst, sobald ihr nächster Schreibversuch auf die bereits geschlossene
Pipe mit `SIGPIPE`/`EPIPE` fehlschlägt — verlässlich genug für den
beabsichtigten Zweck, aber kein hartes Versprechen, und im UI entsprechend
zurückhaltend formuliert (s. Abschnitt 2/4).

## 4. Darstellung im UI

- Indikator an der Aktionskarte (dort, wo Decision-/Risiko-Badges sitzen,
  Spec 0009/0026): dezenter pulsierender Punkt + Text "läuft seit {{s}}s…".
- Button "Verbindung zu diesem Kommando trennen" — deaktiviert und mit
  Zwischentext, sobald geklickt, bis das Ergebnis eintrifft.
- `ActionResultView` (Ergebnisanzeige) zeigt bei `cancelled: true` einen
  zusätzlichen Hinweis, z. B. "⚠ Manuell abgebrochen nach {{s}}s — Ausgabe
  möglicherweise unvollständig, kein regulärer Exit-Code.", statt des
  sonst gezeigten "exit code: …"-Werts.
- Alle neuen Strings über das i18n-System (Spec 0024), keine hartcodierten
  Texte.

## 5. Abgrenzung

- Gilt ausschließlich für `SuggestCommand` über `SshTransport::execute()`/
  `execute_with_stdin()`. `ReadRemoteFile`/`WriteRemoteFile` laufen über die
  separate SFTP-Session (Spec 0020) mit anderer Fehler-/Zeit-Charakteristik
  (SFTP-Operationen sind strukturell selbst-terminierend — kein bekanntes
  "hängt für immer"-Muster wie bei einem interaktiven Exec-Kanal) — nicht
  Teil dieser Spec.
- Kein automatischer Timeout (s. Abschnitt 1).
- Kein `kill -9`-Äquivalent auf Prozessebene (s. Abschnitt 3, letzter
  Absatz) — der Abbruch trennt die Verbindung zum Kommando, tötet nicht
  garantiert den Prozess.

## 6. Offene Punkte

- Soll der 5-Sekunden-Schwellwert später konfigurierbar werden? Aktuell
  fest codiert, analog zu anderen Sparsamkeits-Konstanten in der App (z. B.
  `DEFAULT_MAX_COMMAND_LENGTH`, `SSE_INACTIVITY_TIMEOUT`).
- Automatische Folgerunden (Spec 0021): keine Sonderbehandlung — der
  Indikator erscheint unabhängig davon, ob die Aktion durch eine
  Nutzer-Nachricht oder eine automatische Folgerunde ausgelöst wurde.
- Denkbare spätere Ausbaustufe: derselbe Abbruch-Mechanismus für
  `ProposeNoteUpdate`/`GenerateDocument` wäre sinnlos (kein
  Remote-Kommando dahinter) — bewusst nicht vorgesehen.
