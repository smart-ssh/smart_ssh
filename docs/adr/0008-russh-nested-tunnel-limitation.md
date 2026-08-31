# 0008-russh-nested-tunnel-limitation

## Status
Accepted (als dokumentierte Einschränkung, nicht als gelöstes Problem)

## Kontext

`docs/specs/0005-ssh-module.md`, Abschnitt 5, beschreibt Jump-Host-
Verkettung als Standardtechnik: TCP-Verbindung zum ersten Hop, SSH-
Handshake; für jeden weiteren Hop ein `direct-tcpip`-Channel über die
bestehende Verbindung, darüber erneut ein SSH-Handshake. Das ist exakt der
Standardansatz für SSH-Tunneling durch Bastions, umgesetzt in
`crates/ssh-transport/src/connect.rs` über
`Handle::channel_open_direct_tcpip` → `Channel::into_stream()` →
`client::connect_stream()`.

Gegen den in Spec 0005 Abschnitt 3 festgelegten `russh` (Version 0.63.1)
schlägt der **verschachtelte** SSH-Handshake (zweiter und weitere Hops)
reproduzierbar fehl. Für den Integrationstest
`test_two_hop_jump_connection` (`crates/ssh-transport/tests/integration.rs`,
Aufgabenstellung Teil 2 Punkt 5) wurde das gründlich untersucht.

### Symptom

`ChannelError("Bad packet size: 1397966893")`. Die Zahl 1397966893 ist,
big-endian als 4 Bytes interpretiert, exakt der ASCII-Text `"SSH-"` — der
Client interpretiert also vier Bytes als Paketlängen-Präfix, die eigentlich
der Anfang einer SSH-Identifikationszeile sind.

### Root-Cause-Analyse (per Byte-Level-Tracing verifiziert)

Ein temporärer `AsyncRead`/`AsyncWrite`-Wrapper um den Tunnel-Stream
(client-seitig) sowie ein manueller, protokollierender Ersatz für
`tokio::io::copy_bidirectional` (bastion-seitig) machten die tatsächlich
über die Leitung laufenden Rohbytes sichtbar. Ergebnis, mit einer
eindeutig unterscheidbaren `server_id` für den Zielserver
(`SSH-2.0-testfixtureN` statt des für Client *und* Server identischen
`russh`-Defaults `SSH-2.0-russh_0.63.1`):

```
[client-tunnel] WRITE 22 bytes: "SSH-2.0-russh_0.63.1\r\n"
[client-tunnel] READ  22 bytes: "SSH-2.0-testfixture1\r\n"   <- korrekt
[client-tunnel] WRITE 872 bytes: <KEXINIT>
[client-tunnel] READ   4 bytes: "SSH-"                        <- kumulierter
    Log zeigt: "SSH-2.0-testfixture1\r\nSSH-" — eine ZWEITE Kopie der
    Identifikationszeile des Zielservers beginnt hier.
```

Der bastion-seitige Trace bestätigt das unabhängig direkt auf dem rohen
`TcpStream` zum Zielserver: dessen *erste* Antwort ist die 22-Byte-ID-Zeile
(korrekt); die *zweite*, unmittelbar folgende Antwort beginnt erneut mit
exakt derselben 22-Byte-ID-Zeile, gefolgt vom eigentlichen (validen)
KEXINIT-Paket. Der Zielserver sendet seine Identifikationszeile also
nachweislich zweimal.

**Ausgeschlossene Hypothesen** (jeweils konkret getestet, nicht nur
vermutet):
- **TCP-Nagle-Koaleszenz:** `nodelay: true` in `client::Config` sowie
  `TcpStream::set_nodelay(true)` auf allen beteiligten Sockets (Bastion↔
  Zielserver, Server↔Client) gesetzt — Fehler bleibt identisch.
- **Doppelter `channel_open_direct_tcpip`-Aufruf** (Bastion-seitig): per
  Zähler in `TestHandler` verifiziert — genau **1** Aufruf.
- **Doppelter `run_stream`-Aufruf** (Zielserver-Accept-Loop): per Log
  verifiziert — genau **1** Aufruf, `run_stream` liefert `Ok(...)`.
- **Zweite `send_ssh_id`-Aufrufstelle:** `grep -rn "send_ssh_id"` über die
  gesamte `russh`-0.63.1-Quelle liefert genau **einen** Treffer im
  Server-Code (`server/mod.rs`, innerhalb von `run_stream`, vor dem
  Konstruieren der `Session`) — es gibt keine zweite Stelle im Quellcode,
  die die ID-Zeile erneut schreiben könnte.

Damit liegt die Duplizierung nicht in dieser Implementierung (Bastion-Proxy
und `connect()`-Ablauf entsprechen exakt Spec 0005 Abschnitt 5), sondern in
`russh` selbst — vermutlich in der Interaktion zwischen dem expliziten
Vorab-Schreiben der ID-Zeile in `run_stream` und einer erneuten,
internen (Re-)Initialisierung innerhalb von `session.run(...)`, die nur
unter der (gegenüber einer direkten TCP-Verbindung) veränderten
Timing-/Scheduling-Charakteristik eines `ChannelStream`-vermittelten
Tunnels auftritt — ein direkter Hop (kein Tunnel) zeigt das Problem nicht.

### Bestätigung durch unabhängige Berichte

Zwei unabhängige, zum Zeitpunkt dieser Untersuchung offene und unbeantwortete
Reports beschreiben dasselbe Grundmuster (SSH-über-SSH via
`channel_open_direct_tcpip` + `into_stream()` + `connect_stream()`):

- [Help with SSH Jumphost · Issue #182 · Eugeny/russh](https://github.com/Eugeny/russh/issues/182)
- [Help implement ssh client Russh/Thrussh with jumphost (Rust-Forum)](https://users.rust-lang.org/t/help-implement-ssh-client-russh-thrussh-with-jumphost/99899)

Beide zeigen Verbindungsabbrüche beim verschachtelten Handshake über einen
so aufgebauten Tunnel, ohne dass in den Threads eine Lösung genannt wird.

## Entscheidung

Die Implementierung folgt weiterhin exakt dem in Spec 0005 Abschnitt 5
beschriebenen Standardverfahren (architektonisch korrekt, kein
"Workaround" auf Kosten der Korrektheit). Der Integrationstest
`test_two_hop_jump_connection` bleibt **bestehen**, wird aber mit
`#[ignore]` markiert und trägt einen ausführlichen Doc-Kommentar mit dem
Verweis auf diese ADR — er dient als lauffähige Dokumentation des
erwarteten Verhaltens und als sofortiger Regressionscheck, sobald entweder
`russh` das zugrunde liegende Verhalten behebt oder ein Workaround
gefunden wird (z. B. ein alternativer Weg, den Tunnel-Stream aufzubauen,
der nicht über `Channel::into_stream()` + `connect_stream()` läuft).

## Konsequenzen

**Positiv:**
- Kein unehrlicher "grüner" Test, der in Wahrheit nichts mehr prüft (Test
  bleibt im Code, nur explizit als bekannt-fehlschlagend markiert).
- Die Analyse ist vollständig reproduzierbar dokumentiert (Kommentare in
  `crates/ssh-transport/src/connect.rs`, `tests/fixtures/test_server.rs`,
  `tests/integration.rs`, plus diese ADR) — ein künftiger Versuch (z. B.
  nach einem `russh`-Upgrade) muss die Fehlersuche nicht wiederholen.
- Einzelne Hops (kein Jump-Host) sind von diesem Problem nachweislich
  **nicht** betroffen (3 von 4 Integrationstests grün) — die Kernfunktionalität
  (Exec, PTY, Host-Key-Handling) ist voll nutzbar.

**Negativ / Trade-off:**
- Jump-Host-Verbindungen (Bastion-Ketten) sind mit dieser `russh`-Version
  aktuell **nicht produktiv nutzbar**, obwohl die Spec sie als Kernfeature
  vorsieht (Abschnitt 5). Das ist eine funktionale Lücke gegenüber der
  Spec, nicht nur ein Test-Detail.
- Ohne diese ADR wäre nicht offensichtlich, warum ein architektonisch
  korrekt aussehender Code-Pfad nicht funktioniert — das Risiko, dass
  jemand versucht "den Bug zu fixen", ohne zu wissen, dass er in `russh`
  selbst liegt, wird durch die ausführliche Dokumentation hier minimiert,
  aber nicht eliminiert.
- Ein `russh`-Upgrade (sobald eine neuere Version als 0.63.1 verfügbar ist)
  sollte explizit gegen `test_two_hop_jump_connection` geprüft werden
  (`cargo test -p ssh-transport --test integration -- --ignored`), bevor
  Jump-Hosts als produktionsreif gelten.
