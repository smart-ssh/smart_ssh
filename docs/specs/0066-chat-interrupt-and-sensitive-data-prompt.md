# Spec: Chat jederzeit unterbrechbar + Prompt-Regel für sensible Daten

Status: Umgesetzt
Repo: **öffentlich** `smart_ssh`, `crates/ai-providers` (Abbruch eines
laufenden Streams), `crates/app-shell` (Orchestrierung, Stopp-Command,
System-Prompt), Frontend (`ChatPanel.tsx`)
Abhängigkeiten: Auto-Fortsetzung (0021), max_tokens-Handling (0065, Invariante
"abgeschnittener Tool-Call wird nie ausgeführt"), Filter-Engine/Confirm (0002),
Remote-Datei lesen (0020)

> **Das Problem (von Stefan im echten Einsatz beobachtet):** Nach Klick auf
> „Automatik stoppen" arbeitete die KI weiter, und das Eingabefeld blieb
> gesperrt — eine Korrektur war erst möglich, als die ganze Runde fertig war.
>
> **Ist-Stand (Code geprüft):**
> - `ChatPanel.tsx`: `sending` sperrt Eingabefeld und Senden-Knopf für die
>   **gesamte** Dauer von `send_chat_message` — inklusive aller
>   Auto-Fortsetzungsrunden (der Command läuft synchron über die ganze
>   Rundenkette).
> - `stop_auto_continuation` (`commands.rs`) setzt nur
>   `session.auto_continue_stop`; `orchestration.rs::run_chat_turn` prüft das
>   Flag **nur zwischen Runden**. Ein laufender KI-Stream wird nie
>   abgebrochen; `AiProvider::send()` hat keinerlei Abbruch-Mechanismus.
> - Vorlage für Abbruch existiert: `Transport::execute_cancellable`
>   (`oneshot::Receiver` + `tokio::select!`) und `ConfirmationRegistry`
>   (`cancel_running_command`).
> - Der System-Prompt (`commands.rs::build_session_system_context`) enthält
>   Werkzeug-Anweisungen und den Injection-Hinweis, aber nichts zum Umgang
>   mit sensiblen Dateien. Die Filter-Engine flaggt Lese-Zugriffe auf
>   `~/.ssh/id_*` o. ä. heute nicht.
>
> **Priorität ERHÖHT** — §1/§2 greifen in den Pfad zwischen KI-Vorschlag und
> Ausführung ein (ein abgebrochener Stream darf keinen Tool-Call
> freigeben), §3 ist eine Sicherheitsanweisung an die KI.

## Getroffene Entscheidungen (Stefan)

1. **Senden während die KI arbeitet → Einreihen, als Text mitsenden.** Die
   Nachricht unterbricht nichts, sondern wird dem **nächsten Request an die
   KI** als eigener Text-Block mitgegeben (typisch: zusammen mit dem
   Kommando-Ergebnis der nächsten Auto-Fortsetzungsrunde). Endet der Turn
   ohne weitere Runde, geht sie danach als normale Nachricht raus.
2. **Stopp lässt ein laufendes Remote-Kommando weiterlaufen** — nur die KI
   stoppt. Abbruch des Kommandos bleibt über den bestehenden
   Kommando-Abbruch-Knopf möglich.

## 1. Stopp bricht den laufenden KI-Request ab

- „Automatik stoppen" (bzw. ein neuer „Stopp"-Knopf während einer laufenden
  Antwort) bricht den **gerade laufenden** KI-Stream sofort ab, nicht erst an
  der nächsten Rundengrenze. Der Abbruch schließt die HTTP-Verbindung zum
  Provider (Stream wird gedroppt → reqwest bricht ab; keine weiteren Tokens
  werden erzeugt/bezahlt).
- Mechanismus analog `execute_cancellable`: pro laufendem Chat-Turn ein
  Abbruch-Signal (`oneshot`/`CancellationToken`), `run_one_round` konsumiert
  den Stream per `tokio::select!` gegen dieses Signal. Kein Special-Casing im
  Provider nötig, solange Drop des Streams genügt — falls der 0065-Retry-
  Wrapper (`unfold`) zwischen zwei Versuchen gerade wartet, muss der Abbruch
  auch dort greifen (kein zweiter Request nach Stopp).
- **Invariante:** Aus einem abgebrochenen Stream wird **nie** ein Tool-Call
  (`ActionProposed`) an Confirm/Ausführung weitergegeben — auch dann nicht,
  wenn der Abbruch zeitlich nach einem vollständigen `content_block_stop`
  liegt. (Mit 0065 hält der Provider Tool-Calls bis zum finalen
  `stop_reason` zurück; ein Abbruch vor diesem Punkt verwirft sie
  automatisch — muss per Test abgesichert werden.)
- Bereits gestreamter Text bleibt sichtbar, mit dezenter Markierung
  „Abgebrochen" (UI-Flag, kein Text im Inhalt — Lehre aus 0057, wie der
  Kürzungshinweis in 0065).
- Offener Bestätigungsdialog zum Zeitpunkt des Stopps **bleibt stehen** —
  der Nutzer entscheidet selbst (Ablehnen ist ein Klick). Das ist das
  bestehende Verhalten aus Spec 0021 §5 (per Test abgesichert); ohne
  Bestätigung wird ohnehin nichts ausgeführt. Nach der Entscheidung folgt
  keine weitere Runde.
- Ein schon abgeholter, aber noch nicht gestarteter AutoExec-Vorschlag
  wird nach einem Stopp nicht mehr ausgeführt und als abgebrochen gemeldet
  (nur eigener Chat, nicht MCP; s. ADR 0057 §3).
- Laufendes Remote-Kommando läuft zu Ende (Entscheidung 2); sein Ergebnis
  wird wie gewohnt angezeigt, aber **nicht** mehr automatisch an die KI
  zurückgegeben (keine neue Runde nach Stopp).
- Liegen beim Stopp eingereihte Nachrichten vor (§2), werden sie danach als
  normale neue Nachricht gesendet — der Nutzer wollte sie ja loswerden.

## 2. Nachricht jederzeit senden

- Eingabefeld und Senden-Knopf sind **nie** wegen einer laufenden Antwort
  gesperrt.
- Senden während eines laufenden Turns **reiht ein** (Entscheidung 1): die
  Nachricht erscheint sofort im Chat (markiert als „wird mit der nächsten
  Anfrage gesendet"), der laufende Request wird nicht unterbrochen.
- An der nächsten Rundengrenze wird sie als **eigener Text-Block** in den
  nächsten User-Turn an die KI gelegt — **außerhalb** jeder
  `<stdout>`/`<stderr>`/`<remote_file>`-Umzäunung, klar als Nutzernachricht
  gekennzeichnet. Umgekehrt darf nichts aus Server-Ausgabe als eingereihte
  Nutzernachricht erscheinen können (Nachricht kommt nur aus dem
  Frontend-Command, nie aus Tool-Output).
- Mehrere eingereihte Nachrichten → in Reihenfolge, alle in denselben
  nächsten Request.
- Endet der Turn ohne weitere Runde (KI fertig, Stopp, Fehler, Limit
  erreicht), wird die Warteschlange als **normale** neue Nachricht gesendet.
- Eingereihte Nachrichten durchlaufen denselben Pfad wie normale
  Nachrichten: Redaction vor Persistenz/Versand, Verlauf, Kompaktion, Gate,
  Caching — kein Sonderweg.
- Nebenläufigkeit: pro Session höchstens ein aktiver Chat-Turn; die
  Warteschlange ist der einzige Weg, während eines Turns Text einzuspeisen
  (kein paralleler zweiter Turn, kein paralleles Schreiben in Verlauf/DB).

## 3. Prompt-Regel: sensible Daten nicht lesen

Neuer Absatz im System-Prompt (`build_session_system_context`), sinngemäß:

> Umgang mit sensiblen Daten: Lies den Inhalt von Passwörtern, privaten
> Schlüsseln (z. B. `~/.ssh/id_*`), Tokens, `.env`-Dateien, Zertifikats-Keys
> oder ähnlichen Geheimnissen nur, wenn es wirklich unvermeidbar ist. Willst du
> nur prüfen, ob so eine Datei existiert oder befüllt ist, nutze Metadaten
> (z. B. `test -f`, `stat -c %s`, `ls -l`) statt `cat`. Musst du solche
> Dateien kopieren oder verschieben, tu das direkt auf dem Server (`cp`,
> `install -m 600`, Pipe/Umleitung), statt den Inhalt zu lesen und danach neu
> zu schreiben — so gelangt das Geheimnis nie in den Chat-Verlauf.

- Gilt für Haupt-Chat; Nebenaufrufe (Titel, Notiz, Zusammenfassung,
  Second Opinion) brauchen den Absatz nicht.
- Ist eine **zusätzliche** Vorsichtsmaßnahme, ersetzt weder Redaction noch
  Confirm. Nicht Teil dieser Spec (möglicher Folgeschritt): eine Risiko-
  Eskalation in der Filter-Engine für Lesebefehle auf typische
  Secret-Pfade.
- Test: Prompt enthält den Absatz (Regressionstest gegen Entfernen).

## Abnahme

- Stopp während Streaming → Stream endet < 1 s, kein weiterer Request, Text
  bleibt mit „Abgebrochen"-Markierung.
- Stopp nach vollständigem Tool-Call-Block, aber vor `stop_reason` → kein
  Bestätigungsdialog (Regressionstest).
- Stopp während 0065-Retry-Wartezeit → kein zweiter Request (wiremock
  `.expect(1)`).
- Eingabefeld während Antwort nicht gesperrt; eine währenddessen gesendete
  Nachricht landet im nächsten Request (als eigener Block, außerhalb der
  Fences) bzw. nach Turn-Ende als normale Nachricht.
- Eingereihte Nachricht mit Secret → im Request an die KI redigiert
  (Redaction-Test); in der DB wie jede normale Nutzer-Nachricht
  (verschlüsselt, Spec 0036).
- System-Prompt enthält den Sensible-Daten-Absatz.
