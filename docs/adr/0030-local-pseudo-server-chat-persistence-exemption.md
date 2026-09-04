# 0030-local-pseudo-server-chat-persistence-exemption

## Status
Vorgeschlagen

## Kontext

Spec 0034 führte persistente Chat-Sitzungen ein: jeder `connect()` legt
(außer bei einem Ladefehler, best-effort) eine `chat_sessions`-Zeile an,
referenziert über `chat_sessions.server_id -> servers(id)` als
Fremdschlüssel. Der lokale Pseudo-Server (Spec 0026/0032,
`ssh-transport::LocalTransport`/`app-tauri::local_server`) hat bewusst
**keine** eigene `servers`-Zeile — er ist gerade dafür gebaut, ohne echte
Server-Konfiguration und ohne jede servergebundene Persistenz nutzbar zu
sein (lokales Ausprobieren der KI-Copilot-Funktion ohne SSH-Ziel).

`crate::commands::connect_session` behandelt das explizit:

```rust
} else if is_local {
    (Vec::new(), None)
} else {
    // ... state.chat_session_store.create_session(&server_id, ...)
}
```

Eine lokale Sitzung bekommt also nie eine `chat_session_id`, ihr
`Session::chat_session_store` bleibt `None` — kein Chat-Verlauf wird für
sie persistiert oder ist wiederaufnehmbar. Der Sitzungs-Abgleich (Spec
0040) hat gefragt, ob das eine unbeabsichtigte Lücke ist ("die lokale
Pseudo-Server-Sitzung bekommt keine Chat-Persistenz") oder eine
gewollte Ausnahme.

## Entscheidung

Die Ausnahme ist **gewollt und bleibt bestehen** — keine Codeänderung aus
diesem ADR.

Begründung:

- Ein `INSERT` in `chat_sessions` mit `server_id` = der (nicht
  existierenden) lokalen Pseudo-Server-ID würde die
  `FOREIGN KEY REFERENCES servers(id)`-Einschränkung verletzen (dieselbe
  Grenze, die `docs/architecture-overview` bzw. `docs/adr/0026` für den
  lokalen Pseudo-Server bereits beschreibt: "keine Sonderbehandlung in der
  Kernschleife, aber eine bewusste Grenze an der
  Transport-/Session-Konstruktion"). Die naheliegenden Auswege
  (Fremdschlüssel aufweichen, oder eine künstliche `servers`-Zeile für den
  lokalen Pseudo-Server anlegen) würden beide dem CLAUDE.md-Grundsatz
  widersprechen, dass der lokale Pseudo-Server *nirgends* durch eine
  echte, in der Datenbank sichtbare Server-Identität ersetzt werden soll
  — er hat bewusst keine.
- Der lokale Pseudo-Server dient laut Spec 0026 primär dem
  risikofreien Ausprobieren/Testen der Filter-Engine und des
  KI-Copiloten, nicht der langfristigen, wiederaufnehmbaren
  Zusammenarbeit mit einem echten System — genau der Anwendungsfall, für
  den Chat-Sitzungspersistenz (Spec 0034) gedacht ist, entfällt hier
  konzeptionell.
- Die Ausnahme ist an exakt einer Stelle (`connect_session`s
  `is_local`-Zweig) sichtbar kommentiert, nicht als verstreute
  `if server_id == LOCAL_SERVER_ID`-Prüfung in Filter-Engine, Risiko-
  Klassifizierer oder `orchestration.rs` — entspricht damit
  CLAUDE.mds Vorgabe, dass lokale-Server-Sonderbehandlung ausschließlich
  an der Transport-/Session-Konstruktions-Grenze passieren darf, nie in
  der eigentlichen Sicherheitslogik.

## Konsequenzen

**Positiv:**
- Kein Fremdschlüssel-Konflikt, keine künstliche `servers`-Zeile für ein
  Konstrukt, das bewusst keine ist.
- Konsistent mit der bestehenden, bereits dokumentierten Grenze für den
  lokalen Pseudo-Server (ein einziger Sonderfall an der
  Session-Konstruktion, keine Ausbreitung in die Kernlogik).

**Negativ / Trade-off:**
- Eine Sitzung mit dem lokalen Pseudo-Server ist nach dem Trennen
  unwiederbringlich verloren (kein Eintrag im "Sitzungen fortsetzen"-
  Screen) — für den beabsichtigten Ausprobier-/Test-Anwendungsfall
  akzeptabel, aber ein Nutzer, der den lokalen Pseudo-Server versehentlich
  für eine längere, tatsächlich schützenswerte Unterhaltung nutzt, verliert
  diese beim Schließen des Tabs ohne Vorwarnung. Sollte das künftig relevant
  werden, wäre eine eigene, vom echten `servers`-Fremdschlüssel losgelöste
  Persistenz (z. B. `server_id` nullable machen) eine neue, vom Nutzer zu
  treffende Spec-Entscheidung, kein stiller Nebeneffekt dieses ADRs.
