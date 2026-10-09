# ADR 0120 — MCP-Aktionsergebnisse gefenct wie im KI-Kontext

Status: akzeptiert
Betrifft: Spec 0039, Spec 0028, ADR 0025, ADR 0034, ADR 0103, Issue #34

## Kontext

ADR 0103 fenct die Notizen, die ein externer MCP-Client über
`get_server_notes` liest, und hat die übrigen MCP-Tool-Ergebnisse
ausdrücklich offengelassen. Danach gingen Kommandoausgabe
(`propose_command`) und gelesene Dateien (`read_remote_file`) zwar
redigiert, aber als Klartext an den Client: `stdout:\n…` bzw.
`Inhalt von '<Pfad>':\n\n…`. Ein MCP-Tool fencte also seine nicht
vertrauenswürdigen Inhalte, zwei andere nicht — obwohl gerade
Kommandoausgabe und Dateiinhalt vollständig vom Server kontrolliert werden
und ein externer Client sie ebenso an ein Modell weitergibt wie die
eingebaute KI.

## Optionen

1. **Fencen.** stdout, stderr und Dateiinhalt laufen über
   `fence_untrusted` (`CommandStdout`/`CommandStderr` mit dem Kommando als
   Quelle, `RemoteFile` mit dem Pfad als Quelle) — dieselbe Form wie im
   KI-Kontext. Nachteil: Ein Client, der den bisherigen Klartext parst,
   sieht ein geändertes Format.
2. **Nicht fencen**, nur dokumentieren, warum die MCP-Grenze anders
   behandelt wird als der interne Weg und `get_server_notes`.

## Entscheidung

Option 1.

- `format_action_result` (MCP-Backend der App) fenct stdout immer — auch
  leer, damit die Form gleich bleibt — und stderr, sobald es nicht leer ist,
  sowie den Inhalt eines `fileRead`-Ergebnisses. Es nutzt die bestehende
  `fence_untrusted`, keine zweite Implementierung.
- **Redaction bleibt vor dem Fencing** (ADR 0034). Der
  `chat-action-result`-Payload ist bereits durch den Session-Redactor
  gelaufen, bevor das MCP-Backend ihn sieht; gefenct wird erst beim
  Formatieren für den Client. Ein gieriges Rückfallmuster (abgeschnittener
  Private-Key-Block) kann den schließenden Fence deshalb nicht verschlucken.
- **Statuszeilen bleiben außerhalb.** `Exit-Code: …` und der
  Abbruchhinweis stammen von der App, nicht vom Server, und stehen vor dem
  Fence. Die Zusammenfassungen von `fileWrite` und `noteUpdate` enthalten
  keine vom Server kontrollierten Bytes und bleiben unverändert.
- Die Tool-Beschreibungen von `propose_command` und `read_remote_file`
  sagen dem Client, dass die Ausgabe redigiert und gefenct ist und als
  Daten gilt — wie bei `get_server_notes`.

## Konsequenzen

- Alle drei MCP-Tools, die Inhalte vom Server zurückgeben, behandeln sie
  gleich und gleich wie der interne KI-Kontext. Eine im Server-Output
  eingeschleuste Anweisung oder ein schließendes Tag kann den Fence nicht
  verlassen.
- Das Ausgabeformat zweier MCP-Tools ändert sich: Statt `stdout:`/`stderr:`
  stehen `<stdout>`/`<stderr>`-Fences mit `<source>`, und `<`, `>`, `&` im
  Inhalt sind als Entitäten escapt. Ein Client, der den Text wörtlich
  anzeigt, sieht diese Escapes.
