# ADR 0103 — MCP-Notizen redigiert und gefenct wie im KI-Kontext

Status: akzeptiert
Betrifft: Spec 0039, ADR 0034, Spec 0028 (`get_server_notes`), Issue #18

## Problem

Das MCP-Tool `get_server_notes` gab die effektiven Notizen eines Servers
unverändert an den externen MCP-Client: ohne Redaction, ohne Fence. Die
eingebaute KI bekommt dieselben Notizen dagegen redigiert und als
`<server_note>` gefenct. Spec 0039 hatte externe MCP-Clients ausgeklammert,
seit es `get_server_notes` gibt, sind Notizen aber über MCP lesbar — und ein
externer Client gibt sie ebenso an ein Modell weiter.

## Entscheidung

1. **Je Abschnitt redigieren, dann fencen.** `app_logic::server_redaction::
   redacted_fenced_effective_notes` holt die Abschnitte über
   `effective_notes_sections` (Gruppenkette, dann Server), redigiert jeden
   mit `OutputRedactor::redact_text` und fenct ihn danach mit
   `fence_untrusted(UntrustedKind::ServerNote, <Quelle>, …)`. Abschnitte
   werden wie in `SystemContextParts::assemble_with_notes` mit einer
   Leerzeile verbunden. Die Reihenfolge Redaction → Fencing folgt ADR 0034:
   ein gieriges Fail-safe-Muster darf nie über ein Fence-Tag laufen.
2. **Derselbe Redactor wie in der Sitzung.** Eingebaute Muster plus das
   Sudo-Passwort des Servers (falls hinterlegt). Der Bauplan stand bisher
   zweimal inline in `app_shell::commands::connect` und `…::notes`; er liegt
   jetzt einmal in `server_redaction::redactor_with_sudo_password`, und alle
   drei Aufrufer nutzen ihn. Verhalten von `connect` und `notes` bleibt
   gleich.
3. **Format für den Client:** statt der bisherigen `## Kontext: <Quelle>`-
   Überschriften (aus `effective_notes`) trägt jeder Abschnitt seine Quelle
   im `<source>`-Element des Fence. Keine Notizen ergeben weiterhin einen
   leeren Text. Die Tool-Beschreibung sagt dem Client, dass der Inhalt
   redigiert und gefenct ist und als Daten gilt.
4. **Allow-Liste unverändert zuerst.** Die Prüfung in
   `AppMcpBackend::server_notes` läuft weiterhin vor jedem Lesen.

## Nicht Teil dieser Entscheidung

Die übrigen MCP-Tool-Ergebnisse (Kommandoausgabe, gelesene Dateien) gehen
redigiert, aber nicht gefenct an den Client. Ob sie gefenct werden sollen,
ist eine eigene Entscheidung.

## Konsequenzen

- Ein externer MCP-Client sieht keine Geheimnisse aus Notizen mehr, die der
  Session-Redactor erkennt, und eine in einer Notiz eingeschleuste
  Anweisung kann den Fence nicht schließen.
- Ein Client, der den Notiztext bisher wörtlich angezeigt hat, sieht jetzt
  Fence-Tags und escapte `&lt;`/`&gt;`/`&amp;`.
