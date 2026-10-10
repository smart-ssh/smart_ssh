# 0008-russh-nested-tunnel-limitation

## Status
Superseded / obsolet. Die hier dokumentierte Einschränkung existiert nicht:
Der Fehler lag in der Test-Fixture, nicht in `russh`.

**Update 2026-10-09:** Die Analyse unten ist widerlegt. Jump-Hosts und SFTP
über Jump-Hosts funktionieren, siehe Abschnitt „Update 2026-10-09“ am Ende.
Die Fixture ist behoben (Issue #178): Tunnel-Channels werden nicht mehr
echot, `test_two_hop_jump_connection` läuft und ein neuer Zwei-Hop-Test
deckt PTY und SFTP ab.

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

## Update 2026-10-09 — Nachprüfung (Issue #165): Ursache liegt in der Test-Fixture, nicht in `russh`

**Ergebnis:** SFTP über Jump-Hosts funktioniert. Mit unverändertem
Produktcode und `russh` 0.63.1 laufen Verbindungsaufbau, Exec, PTY und
SFTP (`list_dir`, `read_file`, `write_file` bis 4 MiB) in den Setups
„OpenSSH-Bastion → OpenSSH-Ziel“, „OpenSSH-Bastion → `russh`-Fixture-Ziel“
und „korrigierte Fixture-Bastion → OpenSSH-Ziel“. Der „Bad packet
size“-Fehler entsteht ausschließlich, wenn die **Test-Fixture** aus
`crates/ssh-transport/tests/fixtures/test_server.rs` die Bastion ist. Die
Root-Cause-Analyse und die Konsequenz „Jump-Host-Verbindungen sind nicht
produktiv nutzbar“ weiter oben sind damit **widerlegt**. Sie bleiben als
Historie stehen, gelten aber nicht mehr.

Im Rahmen dieses Updates wurde kein Produktcode, keine Abhängigkeit und
kein Test auf `main` geändert. Alle Experimente liefen auf einem
Wegwerf-Stand und wurden danach verworfen.

### Versuchsaufbau

- Fixture-Server: unverändert aus `tests/fixtures/test_server.rs`
  (`russh`-Server, Passwort `testuser`/`testpass`).
- Echter OpenSSH-`sshd` lokal in Docker (nur lokal, nicht in CI), zwei
  Container in einem gemeinsamen Docker-Netz. Die Bastion ist auf dem Host
  unter `127.0.0.1:2301` erreichbar, das Ziel unter `127.0.0.1:2302`, und
  von der Bastion aus als `exp165-target:22`. Den Fixture-Server auf dem
  Host erreicht die Bastion über `host.docker.internal:<port>`. Image:

  ```dockerfile
  FROM alpine:3.20   # OpenSSH_9.7p1
  RUN apk add --no-cache openssh openssh-sftp-server \
   && adduser -D -s /bin/sh testuser && echo 'testuser:testpass' | chpasswd \
   && ssh-keygen -A \
   && printf 'PasswordAuthentication yes\nKbdInteractiveAuthentication no\nAllowTcpForwarding yes\nSubsystem sftp internal-sftp\nPermitRootLogin no\n' > /etc/ssh/sshd_config
  CMD ["/usr/sbin/sshd","-D","-e"]
  ```

- Wegwerf-Tests in `tests/integration.rs`, `#[ignore]`, Aufruf mit
  `cargo test -p ssh-transport --test integration <name> -- --ignored --exact --nocapture`.
  Sie rufen `ssh_transport::connect()` mit zwei Passwort-Hops auf. Jeder
  Host-Key wird über den normalen Weg bestätigt: `connect()` liefert
  `PendingHostKeyConfirmation` für genau einen Hop, der Test ruft
  `trust()` für diesen Schlüssel auf und verbindet neu. Es gibt keinen
  Pauschal-Trust, die Prüfung je Hop (Spec 0005) bleibt unverändert. In
  jedem Lauf kam für jeden Hop einzeln eine Rückfrage. Danach prüfen die
  Tests `execute("echo via-jump; hostname")`, eine PTY-Shell
  (`open_shell`, `echo pty-$((40+2))`, auf `pty-42` warten, `resize`) und
  SFTP (`open_sftp`, `write_file`, `list_dir`, `read_file` klein, dann
  `write_file`/`read_file` mit 64 KiB, 1 MiB und 4 MiB samt Byte-Vergleich).
  Jeder Schritt hat ein Timeout.

### 1. Tritt der Zwei-Hop-Fehler auf dem aktuellen `main` noch auf?

Ja. `cargo test -p ssh-transport --test integration -- --ignored` auf
`main` (`russh` 0.63.1):

```
test test_two_hop_jump_connection ... FAILED
Zwei-Hop-connect() sollte gelingen: ChannelError("Bad packet size: 1397966893")
```

### 2. Nur mit dem Fixture-Server oder auch mit echtem OpenSSH?

Nur, wenn die Fixture die **Bastion** ist. Ob das Ziel die Fixture ist,
spielt keine Rolle.

| Bastion | Ziel | Fixture original | Fixture korrigiert (s. 5.) |
|---|---|---|---|
| Fixture | Fixture | ❌ `Bad packet size: 1397966893` | ✅ |
| OpenSSH 9.7p1 | OpenSSH 9.7p1 | ✅ (Fixture nicht beteiligt) | ✅ |
| OpenSSH 9.7p1 | Fixture | ✅ | ✅ |
| Fixture | OpenSSH 9.7p1 | ❌ `ChannelError("Key exchange init failed")` | ✅ |
| — (ein Hop) | OpenSSH 9.7p1 | ✅ | ✅ |

Die Zeile „OpenSSH-Bastion → Fixture-Ziel“ widerlegt die frühere
Annahme, der Fixture-Zielserver sende seine Identifikationszeile zweimal.
Hinter einer korrekt weiterleitenden Bastion verhält er sich korrekt.

### 3. Behebt eine neuere `russh`-Version den Fehler?

Nein, sie muss es auch nicht, denn der Fehler liegt nicht in `russh`.
Auf einem Wegwerf-Stand getestet:

| `russh` | Wie eingestellt | `test_two_hop_jump_connection`, Fixture original | dito, Fixture korrigiert |
|---|---|---|---|
| 0.63.1 (Pin auf `main`) | — | ❌ `Bad packet size: 1397966893` | ✅ |
| 0.63.2 | `cargo update -p russh --precise 0.63.2` | ❌ `Bad packet size: 1397966893` | nicht separat geprüft |
| 0.64.1 (neueste) | `russh = "0.64.1"` in `crates/ssh-transport/Cargo.toml`, `cargo update -p russh` | ❌ `Bad packet size: 1397966893` | ✅ |

`russh` 0.64.1 kompiliert ohne Codeänderung. Unter 0.64.1 läuft
`cargo test -p ssh-transport` grün (60 Unit-Tests, 37 Integrationstests),
und die OpenSSH-Setups aus 2. laufen ebenfalls (inklusive SFTP mit
4 MiB). Ein Upgrade ist für Jump-Hosts also nicht nötig, steht dem aber
auch nicht im Weg.

### 4. Funktionieren Terminal und SFTP über den Jump-Host?

Ja, in jedem Setup, in dem die Verbindung zustande kommt (✅ in 2.):

- **Exec:** `execute()` liefert die erwartete Ausgabe vom **Zielhost**
  (bei OpenSSH `via-jump` und der Hostname des Ziel-Containers),
  Exit-Code 0.
- **PTY:** `open_shell()` öffnet die Shell, `echo pty-$((40+2))` liefert
  `pty-42`, `resize()` gelingt.
- **SFTP:** `open_sftp()`, `write_file()`, `list_dir()` (zeigt die eben
  geschriebene Datei im Home des Ziels) und `read_file()` funktionieren.
  Gegen OpenSSH gelingen `write_file`/`read_file` mit 64 KiB, 1 MiB und
  4 MiB byte-genau, sowohl über den Jump-Host als auch direkt. Exec
  meldet über den Jump-Host denselben Hostnamen wie bei einer direkten
  Verbindung zum Ziel-Container. Die Befehle laufen also auf dem Ziel und
  nicht auf der Bastion.

SFTP über einen Jump-Host ist kein eigener Pfad: Das Subsystem öffnet
sich auf der bestehenden `SshTransport`-Verbindung (Spec 0020 §3). Diese
Experimente liefen eine Ebene unter der App-Logik, also ohne die
Bestätigung nach Spec 0020 §4. Dieser Weg ist unabhängig von der Zahl der
Hops und wurde hier nicht verändert.

Nebenbefund, unabhängig von Jump-Hosts: Gegen das SFTP-Subsystem der
**Fixture** hängt `write_file` ab 1 MiB, auch bei einer direkten
Verbindung ohne Jump-Host (64 KiB und 256 KiB gehen). Gegen OpenSSH tritt
das nicht auf. Es liegt also an der Fixture und nicht am Produkt, ist aber
nicht weiter untersucht.

### 5. Ursache und Lösungsoptionen

**Ursache:** Ein `russh`-Server leitet eingehende `CHANNEL_DATA` sowohl an
den `Channel` bzw. dessen `ChannelStream` weiter als auch an
`Handler::data()`. Das ist in `russh` 0.63.1 so (`server/encrypted.rs`,
Zweig `msg::CHANNEL_DATA`) und ebenso in 0.64.1. Der `TestHandler` der
Fixture implementiert `data()` als Echo für alle Kanäle („Echo-Shell“ für
die PTY-Tests), also auch für den `direct-tcpip`-Tunnelkanal, den er per
`channel.into_stream()` + `copy_bidirectional` zum Ziel weiterleitet.
Jedes Byte, das der Client in den Tunnel schreibt, geht deshalb zum Ziel
**und** als Echo zurück an den Client:

1. Der Client schreibt `SSH-2.0-russh_0.63.1\r\n` (22 Bytes) in den Tunnel.
2. Das Ziel antwortet mit `SSH-2.0-testfixture1\r\n` (zufällig ebenfalls
   22 Bytes). Der Client liest diese Zeile korrekt.
3. Danach liest der Client das **Echo seiner eigenen** Identifikationszeile
   und deutet die ersten vier Bytes `SSH-` als Paketlänge
   (`0x5353482D` = 1397966893). Daher kommt `Bad packet size`. Ist das Ziel
   ein OpenSSH, scheitert der Client mit einer anderen Meldung
   (`Key exchange init failed`). Auch das verschwindet mit der korrigierten
   Fixture, die Ursache ist also dieselbe.

Die frühere Deutung „das Ziel sendet seine ID-Zeile zweimal“ beruhte auf
dem Präfix `SSH-`, das zu beiden Zeilen passt. Sie ist durch 2.
widerlegt. Dass der Fehler nur über einen Tunnel auftrat, erklärt sich
ebenfalls: Bei einem direkten Hop gibt es keinen `direct-tcpip`-Kanal,
der ein Echo erzeugen könnte.

**Gegenbeweis:** Die Echo-Shell der Fixture überspringt Tunnelkanäle
(4 Zeilen, nur im Test-Code). Damit wird
`test_two_hop_jump_connection` unter `russh` 0.63.1 und 0.64.1 grün,
ohne Änderung am Produktcode. Mit der ursprünglichen Fixture ist er
wieder rot. Der Wegwerf-Patch:

```diff
 struct TestHandler {
     channels: HashMap<ChannelId, Channel<Msg>>,
+    tunnel_channels: std::collections::HashSet<ChannelId>,
 …
     async fn data(&mut self, channel: ChannelId, data: &[u8], session: &mut Session) -> … {
+        if self.tunnel_channels.contains(&channel) { return Ok(()); }
         session.data(channel, data.to_vec())?;
 …
     // channel_open_direct_tcpip, nach erfolgreichem TcpStream::connect:
+    self.tunnel_channels.insert(channel.id());
     reply.accept().await;
```

Ein Produktfehler liegt nicht vor: `ssh-transport` enthält keinen
`russh`-Server, und der `ClientHandler` überschreibt `data()` nicht. Die
Bastion ist im Produkt immer ein fremder `sshd`.

**Optionen:**

| Option | Aufwand | Risiko | Bewertung |
|---|---|---|---|
| a) Fixture korrigieren (Patch oben), `#[ignore]` von `test_two_hop_jump_connection` entfernen, einen Zwei-Hop-Test für PTY und SFTP (`list_dir`/`read_file`/`write_file`) ergänzen, die falsche Ursachenbeschreibung in den Doc-Kommentaren von `connect.rs`, `tests/integration.rs` und `tests/fixtures/test_server.rs` korrigieren, Status dieser ADR auf „überholt“ setzen | klein, etwa 2 bis 3 Stunden | niedrig: nur Test-Code und Kommentare, kein Verhalten ändert sich | **empfohlen** |
| b) `russh`-Upgrade auf 0.64.x | klein (kompiliert ohne Änderung) | mittel (sicherheitsrelevante Abhängigkeit, Changelog prüfen) | für Jump-Hosts nicht nötig, eigenes Thema |
| c) Workaround in `connect.rs` (z. B. eigener Tunnel-Stream statt `into_stream()`) | mittel | mittel bis hoch (berührt den Verbindungsaufbau und die Host-Key-Prüfung je Hop) | **nicht** sinnvoll, es gibt nichts zu umgehen |
| d) Upstream-Patch in `russh` | — | — | entfällt, `russh` verhält sich wie dokumentiert |
| e) Optionaler CI-Job mit echtem OpenSSH-`sshd` (Container wie oben) als End-to-End-Test für Jump-Hosts | mittel | niedrig, aber CI-Laufzeit und Docker-Abhängigkeit (vor allem auf dem macOS- und Windows-Runner) | optional, später; Option a) deckt den Regressionsschutz bereits ab |

Dazu kommt ein eigener Folgepunkt für den Nebenbefund aus 4. (Fixture-SFTP
hängt bei großen Schreibvorgängen).

## Update — Option e) umgesetzt (Issue #180): opt-in End-to-End-Tests gegen OpenSSH

Die Tests in `crates/ssh-transport/tests/openssh_jump.rs` prüfen Exec, PTY
(`open_shell`, `resize`) und SFTP (`write_file`, `list_dir`, `read_file`,
byte-genau bis 4 MiB) über Bastion → Ziel gegen zwei echte OpenSSH-Container
(`tests/openssh/Dockerfile`, dasselbe Image wie oben). Sie sind `#[ignore]`
und brauchen Docker; lokal startet
`crates/ssh-transport/tests/openssh/run-jump-tests.sh` Container und Tests.
Die Host-Keys werden je Hop über `PendingHostKeyConfirmation` bestätigt.

Entschieden (Annahmen, vom Issue empfohlen): Der CI-Job läuft nur auf
`ubuntu-latest`, bei Pull Requests, die `crates/ssh-transport/**` berühren,
sowie täglich und manuell; er ist zunächst nicht blockierend. Die
Test-Fixture ist weiterhin keine geeignete Bastion (siehe oben); die Tests
verwenden sie deshalb nicht.
