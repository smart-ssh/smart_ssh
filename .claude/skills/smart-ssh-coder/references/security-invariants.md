# Sicherheitsinvarianten (Stand der bisherigen Specs/ADRs)

Jede Regel mit Herkunft. Vor Änderungen am jeweiligen Bereich die genannte
Spec/ADR lesen. Neue Invarianten aus neuen Specs hier ergänzen.

## KI-Vorschläge → Server

1. **Jede KI-Aktion läuft durch Filter-Engine + Bestätigung.** Kein Pfad
   von einem KI-Vorschlag zur Ausführung ohne `handle_action_proposed` →
   `evaluate_action` (`crates/app-shell/src/orchestration.rs`).
2. **Eskalation nur in eine Richtung.** Regelbasiertes `Deny`/Risiko-`Red`
   wird nie durch KI-Meinung oder neuen Code aufgeweicht
   (ADR 0024, CLAUDE.md).
3. **MCP-Aktionen verlangen immer Bestätigung** (AutoExec → Confirm,
   `FILTER_MCP_ORIGIN_REQUIRES_CONFIRM`). Tests mit MCP-Herkunft brauchen
   deshalb einen Responder, der den Dialog auflöst.
4. **Automatisch gewonnenes Vertrauen endet nie im Ausführen ohne Grund**:
   Folge-Runden (Spec 0021) sehen Ergebnisse automatisch, führen aber
   nichts automatisch aus, was nicht erneut durch Filter/Confirm lief.

## Streaming, Abbruch, Kürzung

5. **Ein Tool-Call aus einer abgeschnittenen/unvollständigen Antwort wird
   nie ausgeführt oder angezeigt** — Provider halten Tool-Calls bis zum
   finalen `stop_reason`/`finish_reason` zurück; nur eine **Allowlist**
   bekannter Erfolgsgründe zählt als vollständig (Spec 0065, ADR 0056).
   Einmaliger Retry mit mehr `max_tokens`, dann sichtbarer Fehler.
6. **Stopp gewinnt**: `run_one_round` wählt per `biased tokio::select!`
   zuerst den Stopp; aus einem abgebrochenen Stream geht nichts an
   Filter/Confirm. Ein schon abgeholter, noch nicht gestarteter
   AutoExec-Vorschlag wird nach Stopp nicht ausgeführt (nur eigener Chat,
   nicht MCP). Laufende Kommandos laufen zu Ende, offene Dialoge bleiben
   stehen (Spec 0066, ADR 0057).
7. **Stopp-Flag-Reset atomar mit Turn-Start** (unter dem
   `chat_turn`-Lock in `send_chat_message_impl`) — sonst verschluckt ein
   später Reset einen frühen Stopp (Spec 0066).

## Nicht vertrauenswürdiger Inhalt

8. **Server-Ausgabe, gelesene Dateien, Notizen, Systemkennung sind
   gefencet** (`<stdout>`, `<stderr>`, `<remote_file>`, `<server_note>`,
   `<remote_system>`) und gelten nie als Anweisung (Spec 0039/0057).
9. **UI-Hinweise sind Events/Flags, nie Text im Inhalt** — z. B.
   "abgeschnitten" (`chat-response-truncated`), "abgebrochen"
   (`chat-response-cancelled`). Ein Hinweis im Text wäre von Modellausgabe
   nicht unterscheidbar (Lehre aus Spec 0057).
10. **Eingereihte Nutzer-Nachrichten** (Spec 0066) stammen ausschließlich
    aus dem Frontend-Command, nie aus Tool-Output, und laufen über
    denselben Pfad wie normale Nachrichten.

## Redaction und Geheimnisse

11. **Redaction vor Versand/Log**: `reapply_redaction_for_send` vor jedem
    `AiProvider::send()`; Kommando-Ausgaben werden beim Erzeugen redigiert.
    Nutzertext wird verschlüsselt gespeichert (Spec 0036) und beim Versand
    redigiert. Test-Muster: Geheimnis einpflanzen (`AKIAABCDEFGHIJKLMNOP`
    wird erkannt), prüfen, dass es nie im Klartext im Request steht.
12. **System-Prompt-Regel**: die KI soll Geheimnisse nicht lesen,
    Existenz über Metadaten prüfen, Kopien direkt auf dem Server machen
    (Spec 0066 §3, `build_session_system_context`).
13. **Probe-/sudo-stderr nie in Chat/KI-Kontext** — nur in die Meldung
    des Dateibrowsers (Spec 0067).

## Manuelle Aktionen (Dateibrowser, Terminal)

14. **Manuelle Aktionen laufen nicht durch Filter-/KI-Code** — auch nicht
    durch Hilfsfunktionen davon (Spec 0054). Server-verändernde Aktionen
    sind **audit-erfassbar**: je ein schmaler, benannter Tauri-Command,
    Quelle "manuell" (ADR 0045).
15. **Erhöhter Dateibrowser (Spec 0067)**:
    - eigener Kanal `Session::elevated_sftp`, erreichbar nur mit
      `commands::BrowserAccess` (privater Konstruktor in `commands.rs`) —
      KI (`orchestration.rs`) und MCP (`mcp_backend.rs`) können ihn nicht
      einmal kompilierbar ansprechen. Regressionstest:
      `test_ai_and_mcp_file_actions_never_use_the_elevated_channel`.
    - nie Default, nie persistiert, beim Öffnen des Browsers und bei
      Verbindungsverlust aus, unübersehbar gekennzeichnet, Bestätigungen
      nennen den Modus.
    - **kein stiller Rückfall**: eine als erhöht angeforderte Aktion ohne
      aktiven Kanal scheitert.
    - nur `sudo -n` (nie ein Passwort), Pfade/Nutzer streng validiert
      (landen ungequotet im Kommando), nie hängen (Timeouts).
    - Temp-Dateien des Bearbeiten-Flows 0600 ab Erzeugung, Ordner 0700.

## Architektur-Invarianten

16. **`core` ohne Tauri/UI**; neue externe Abhängigkeit → erst Trait.
17. **Kein Sonderfall für den lokalen Pseudo-Server in Sicherheitslogik** —
    Unterschiede an der Transport-/Session-Grenze (z. B. Trait-Default
    "nicht unterstützt").
