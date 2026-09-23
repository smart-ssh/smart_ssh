# Spec 0075 — Server-Profile aus `ssh_config` importieren und dorthin exportieren

Status: Vorschlag (Architekt) · Backlog: BL-0216, BL-0218 · Gate: release-1.0/D
Repo: **öffentlich** `smart-ssh` — `crates/core/src/profiles/` (Abbildung),
`crates/app-shell/src/` (Kommandos, Dateizugriff, DTOs),
`crates/persistence-sqlite/` (ein Feld), Frontend (Vorschau-Dialog)
Review-Priorität: **ERHÖHT** (der Import erzeugt Server-Profile — die
Objekte, gegen die später Filterregeln, Risikobewertung und der
Ausführungspfad laufen — und er öffnet Dateien, deren Pfade aus einer
fremden Datei stammen; adversariale Fälle in §6.4)

> Wer heute umsteigt, tippt jeden Server ab; wer geht, nimmt nichts mit.
> Beides ändert eine Datei, die jedes Werkzeug liest und die **keine
> Geheimnisse enthält**: `ssh_config`. Import und Export gehören in eine
> Spec, weil derselbe Parser in beide Richtungen läuft und der Rundlauf
> der einzige Test ist, der beide Seiten gleichzeitig prüft.

## 1. Ausgangslage (belegt)

**Es gibt heute keinen Import und keinen Export.** Gemessen am
2026-09-23:

```
$ grep -rln "ssh_config|\.ssh/config|SshConfig" --include="*.rs" .
crates/core/src/risk/tests.rs
```

Der einzige Treffer ist `crates/core/src/risk/tests.rs:547` — dort steht
`"cat ~/.ssh/config"` als Beispielkommando in einer Filter-Testvorlage.
Kein produktiver Lese- oder Schreibpfad.

### 1.1 Das Datenmodell, das der Import füllen muss

`Server` (`crates/core/src/profiles/types.rs:44-79`), die für diese Spec
erheblichen Felder:

| Feld | Zeile | Bedeutung hier |
|---|---|---|
| `name: String` | 46 | Anzeigename — der `Host`-Alias |
| `host: String` | 47 | Adresse — `HostName` |
| `port: u16` | 48 | `Port` |
| `username: String` | 49 | `User` |
| `group_id: Option<GroupId>` | 50 | Gruppe — §4.5 |
| `tags: Vec<String>` | 57 | Platzhalter-Zugehörigkeit — §4.5 |
| `auth: AuthMethod` | 58 | §1.2 |
| `notes: String` | 60 | unberührt |
| `jump_host: Option<ServerId>` | 62 | „Bastion/Jump-Host-Verkettung" — `ProxyJump` |
| `post_ingest_policy` | 67 | Sicherheitsverhalten, **nie aus der Datei** |
| `ai_injection_check_enabled: bool` | 73 | ebenso |
| `sftp_server_path: Option<String>` | 76 | unberührt |

`Group` (`types.rs:32-40`) ist über `parent_id: Option<GroupId>`
verschachtelbar; die Kette liest `ProfileStore::group_chain`
(`crates/core/src/profiles/store.rs:107`). Auf dieser Verschachtelung
setzt §4.5 auf.

### 1.2 `IdentityFile` passt nicht ins Modell — das ist der Kern

`AuthMethod` (`types.rs:114-127`) kennt vier Formen:
`Password { credential_ref }`, `PrivateKey { credential_ref,
passphrase_ref }`, `Agent`, `Certificate { cert_ref, key_ref }`. Jedes
`CredentialRef` (`types.rs:137`) ist ein **undurchsichtiger
Schlüsselbund-Schlüssel**, kein Pfad.

Und was dahinter liegt, ist der Schlüssel **selbst**, nicht sein Ort. Die
Eingabe heißt `key_content` (`crates/app-shell/src/dto.rs`,
`AuthMethodInput::PrivateKey { key_content, passphrase }`), und beim
Verbinden wird genau dieser Inhalt geparst:

```rust
// crates/ssh-transport/src/auth.rs:69
let parsed = PrivateKey::from_openssh(key.expose_secret().as_bytes())
```

**Damit gibt es in Smart SSH heute keine dateibasierte
Schlüssel-Authentifizierung.** `ssh_config` kennt aber nur den Pfad.
Entschieden (§9, E-2): Der Pfad wird in einem neuen Feld **gemerkt und
exportiert, aber von dieser Spec nicht zur Anmeldung benutzt**; die
dateibasierte Anmeldung folgt unmittelbar als **Spec 0076**. §3.1.9 und
§4.6 halten fest, was die Oberfläche deshalb sagen muss.

### 1.3 Was beim Anlegen eines Servers **nicht** geprüft wird

Gemessen in `crates/app-shell/src/servers.rs:33-80` und
`crates/app-shell/src/dto.rs:476-505`:

- **Kein Pflichtfeld-Check.** Weder `name` noch `host` werden auf
  nicht-leer geprüft.
- **Keine Namenseindeutigkeit.** Kein Konflikt-Check, keine
  `UNIQUE`-Einschränkung.
- **Keine Zyklusprüfung der Jump-Host-Kette.** Sie existiert nur beim
  Verbindungsaufbau: `crates/core/src/ssh/jump_host.rs:27-29`,
  `if !visited.insert(jump_id) { return Err(SshError::JumpHostCycle); }`.
- Geprüft wird: `normalize_sftp_server_path` (servers.rs:41), die
  Auflösung der Geheimnisse in den Schlüsselbund mit Rollback
  (servers.rs:44-54, 75-78) und — im dünnen Wrapper, nicht in
  `create_server` — `reject_local_jump_host`
  (`crates/app-shell/src/commands.rs:2527-2535`, Code
  `SERVER_JUMP_HOST_LOCAL`).

Das ist heute vertretbar, weil jeder Server einzeln von Hand entsteht.
**Ein Import kippt diese Voraussetzung**: Eine Datei aus fremder Hand
legt beliebig viele Profile auf einmal an. Die Prüfungen, die bisher der
Mensch im Formular war, muss deshalb der Import mitbringen (§3.1.7,
§3.1.8).

### 1.4 Vorhandene Wege, auf denen diese Spec aufsetzt

- Der lokale Pseudo-Server ist `ServerId(Uuid::nil())`, erkennbar an
  `app_shell::local_server::is_local` (`local_server.rs:34`).
- Ein zusätzliches Server-Feld ist im Haus üblich:
  `0014_server_sftp_server_path.sql` ist genau dieser Fall (nullable
  Spalte, Wanderung ohne Datenumbau). Die nächste freie Nummer ist
  `0015`.

## 2. Ziel und Nicht-Ziele

**Ziel.** Ein Nutzer kann eine `ssh_config` einlesen, vorher sehen, was
entstehen würde, und seine Server wieder hinausschreiben. Der Rundlauf
ergibt dieselben Profile.

**Nicht-Ziele** — ausdrücklich, damit nicht mitrepariert wird:

1. **Kein Zugriff auf Termius-Interna.** Termius hat keine dokumentierte
   Exportfunktion; die Community-Werkzeuge lesen seine verschlüsselte
   Electron-IndexedDB über einen Remote-Debugging-Port. Darauf setzt hier
   nichts auf, und empfohlen wird es auch nicht. Wir lesen Dateien, die
   der Nutzer selbst erzeugt (BL-0216).
2. **Kein CSV-Import.** Das ist BL-0217 und hängt an einer eigenen
   Entscheidung über Geheimnisse im Klartext.
3. **Kein verschlüsseltes Eigenformat.** Das ist BL-0081.
4. **Kein Einlesen von Schlüsseldateien.** Der `IdentityFile`-Pfad wird
   gespeichert und exportiert, aber die Datei dahinter wird von dieser
   Spec **nie geöffnet**. Das Einlesen in den Schlüsselbund ist eine
   ausdrückliche, vom Nutzer ausgelöste Handlung und gehört in **Spec
   0076** (§1.2, §4.6, §9/E-2 und E-5) — nicht in einen Vorgang, der
   eine fremde Datei verarbeitet.
5. **Kein Geheimnis in keiner Richtung.** Weder liest der Import eine
   Schlüsseldatei, noch schreibt der Export ein Passwort, eine
   Passphrase oder Schlüsselmaterial.
6. **Keine Verwaltung der echten `~/.ssh/config`.** Wir schreiben nie in
   die Datei des Nutzers (§3.2.5).

## 3. Anforderungen

### 3.1 Teil A — Import (BL-0216)

**3.1.1** Der Import liest eine vom Nutzer im Dateidialog gewählte Datei
im OpenSSH-`ssh_config`-Format. Er liest **keine** Datei von sich aus,
auch nicht `~/.ssh/config`. Weitere Dateien öffnet er nur über
`Include` nach §3.1.4.

**3.1.2** Übernommen werden MUSS mindestens:

| Direktive | Ziel | Anmerkung |
|---|---|---|
| `Host <alias>` | `name` | je Block ein Profil |
| `HostName` | `host` | fehlt sie, gilt der Alias als Adresse (`ssh`-Verhalten) |
| `Port` | `port` | fehlt sie: 22 |
| `User` | `username` | fehlt sie: leer |
| `ProxyJump` | `jump_host` | §3.1.6 |
| `IdentityFile` | `identity_file` | **nie eingelesen**, §3.1.9 |
| `Include` | Untergruppe | §3.1.4 |

**3.1.3 Platzhalterblöcke werden Vorgaben und Schlagworte, keine
Profile.** Ein `Host`-Muster mit `*`, `?` oder `!` beschreibt keinen
einzelnen Server und wird **nicht** als Profil angelegt. Stattdessen:

- Seine Werte gelten als Vorgabe für alle passenden konkreten Blöcke,
  nach der OpenSSH-Regel „**der erste gewinnt**": Der zuerst gelesene
  Wert für eine Direktive bleibt stehen.
- Jeder passende Server erhält das Muster als **Schlagwort** (`tags`),
  wörtlich, also `*.prod.de`. Passt ein Server auf mehrere Muster, erhält
  er mehrere Schlagworte. **Aus einem Platzhalterblock entsteht niemals
  eine Gruppe** — Gruppen kommen ausschließlich aus Dateien (§4.5).
- Das Muster `Host *` allein erzeugt **kein** Schlagwort; es passt auf
  alles und trägt keine Information.

Die Vorschau zeigt bei jedem betroffenen Feld, aus welchem Block der
Wert stammt.

**3.1.4 `Include` wird gefolgt, und jede eingebundene Datei wird eine
Untergruppe.** Entschieden in §9/E-4.

- **Pfade werden nicht eingeschränkt.** Absolute Pfade, `~` und `..`
  sind zulässig. Ein relativer Pfad wird relativ zum **Verzeichnis der
  einbindenden Datei** aufgelöst — nicht relativ zu `~/.ssh`, wie
  OpenSSH es für die Nutzerkonfiguration tut, weil die gewählte Datei
  eben nicht `~/.ssh/config` sein muss. Diese Abweichung steht in der
  Vorschau.
- **Platzhalter im Pfad** (`Include conf.d/*.conf`) werden aufgelöst;
  jede getroffene Datei zählt einzeln.
- **Tiefe höchstens 3.** Die gewählte Datei ist Tiefe 0; ein `Include`
  darin führt auf Tiefe 1. Ein `Include` auf Tiefe 3 wird nicht mehr
  gefolgt, sondern als nicht übernommen gemeldet (3.1.5).
- **Keine Anzahlgrenze für Dateien** — wohl aber die Gesamtgrenzen aus
  §3.3, die über alle Dateien zusammen gelten (§9/E-4).
- **Keine Schleifen.** Jede Datei wird je Import **höchstens einmal**
  gelesen; entschieden über den aufgelösten, kanonischen Pfad. Ein
  zweites Vorkommen wird übersprungen und gemeldet — das erschlägt sowohl
  den Selbstbezug als auch die Kette über mehrere Dateien.
- **Jede eingebundene Datei wird eine Untergruppe**, benannt nach dem
  Dateinamen, mit der Gruppe der einbindenden Datei als Elternteil
  (§4.5).
- **Die Vorschau nennt jede gelesene Datei mit vollem Pfad**, in der
  Reihenfolge, in der sie gelesen wurde. Eine Datei, die nicht lesbar
  ist, lässt den Import nicht scheitern: Sie wird übersprungen und
  gemeldet.

**3.1.5 Nicht übernommene Direktiven werden sichtbar gemeldet**, je
Block und Direktive, mit **Datei und Zeilennummer**. Stillschweigend
verschlucken ist ausgeschlossen (BL-0216, Akzeptanz).

**3.1.6 `ProxyJump`** bildet auf `jump_host: Option<ServerId>` ab:

- Ein einzelner Hop, dessen Ziel im selben Import (über alle Dateien
  hinweg) oder bereits im Bestand vorkommt → `jump_host` zeigt darauf.
- **Mehrere Hops** (`ProxyJump a,b,c`): Unser Modell kettet über je ein
  `jump_host` pro Server, kann die Kette also darstellen — aber nur,
  wenn alle Zwischenstationen als Profile existieren. Der Import MUSS
  die Kette in dieser Form anlegen, wenn alle Glieder auflösbar sind,
  und sie sonst als **nicht übernommen** melden (3.1.5), statt sie zu
  kürzen.
- Ein Hop, der sich nicht auflösen lässt (Host weder in einer der
  gelesenen Dateien noch im Bestand, oder `user@host:port`-Form ohne
  zugehöriges Profil) → nicht übernommen, gemeldet.
- `ProxyJump none` → kein Jump-Host.
- Zeigt ein Hop auf den lokalen Pseudo-Server, wird er abgelehnt
  (bestehende Regel `SERVER_JUMP_HOST_LOCAL`).

**3.1.7 Vorschau vor dem Anlegen.** Der Nutzer sieht die vollständige
Liste der Profile, die entstehen würden, samt Feldern, Herkunft der
Werte (3.1.3), gelesenen Dateien (3.1.4), nicht übernommenen Direktiven
(3.1.5), entstehender Gruppenstruktur (§4.5) und Konflikten (3.1.8),
**bevor** irgendetwas geschrieben wird. Er kann einzelne Einträge
abwählen. Bricht er ab, entsteht nichts.

**3.1.8 Kein Überschreiben.** Existiert bereits ein Server mit gleichem
`name` **oder** gleicher Kombination aus `host`, `port` und `username`,
wird nicht überschrieben. Die Vorschau zeigt den Konflikt; der Nutzer
wählt je Eintrag: überspringen (Vorgabe) oder als neues Profil mit
abweichendem Namen anlegen.

**3.1.9 `IdentityFile` wird gemerkt, nicht gelesen und nicht benutzt.**
Der Pfad landet unverändert im neuen Feld `identity_file` (§4.6). Die
Datei, auf die er zeigt, wird **nicht geöffnet**. Die
Authentifizierungsart des angelegten Servers ist `AuthMethod::Agent`.
Die Oberfläche MUSS am Feld erkennbar machen, dass der Pfad angezeigt
und exportiert, aber **nicht zur Anmeldung benutzt** wird, und auf Spec
0076 verweisen. Mehrere `IdentityFile`-Zeilen in einem Block: Der erste
Wert gewinnt, die weiteren werden als nicht übernommen gemeldet.

**3.1.10 Wiederholbarkeit.** Ein zweiter Import derselben Datei ohne
zwischenzeitliche Änderung legt **nichts** an (alles Konflikt, alles
Vorgabe „überspringen") — auch keine zweite Gruppe.

**3.1.11 Alles oder nichts je Bestätigung.** Schlägt das Anlegen mitten
in einem bestätigten Import fehl, bleibt kein halb angelegter Zustand:
Bereits erzeugte Profile **und Gruppen** dieses Laufs werden
zurückgenommen, und die Meldung nennt den fehlgeschlagenen Eintrag. Das
entspricht dem bestehenden Rollback-Verhalten in `servers.rs:44-54`.

**3.1.12 Keine Sicherheitseinstellung aus der Datei.** Der Import setzt
`post_ingest_policy` und `ai_injection_check_enabled` **ausschließlich**
auf die Vorgabewerte des Produkts. Keine Direktive und kein Kommentar
darf sie beeinflussen (§5.2).

**3.1.13** Der Import MUSS in einer verständlichen Meldung scheitern und
nichts anlegen, wenn: die gewählte Datei nicht lesbar ist, in keiner
gelesenen Datei ein `Host`-Block gefunden wird, oder eine Grenze aus
§3.3 überschritten ist.

### 3.2 Teil B — Export (BL-0218)

**3.2.1** Exportiert werden je Server: `Host <name>`, `HostName <host>`,
`Port <port>` (wenn ≠ 22), `User <username>` (wenn nicht leer),
`ProxyJump <name des Jump-Hosts>` (wenn gesetzt) und `IdentityFile
<pfad>` (wenn `identity_file` gesetzt).

**3.2.2 Kein Geheimnis in der Datei.** Nie ein Passwort, nie eine
Passphrase, nie Schlüsselmaterial. Ein Server mit
`AuthMethod::Password` verliert beim Export seine Anmeldung — das ist
beabsichtigt und wird als Kommentar vermerkt (3.2.3).

**3.2.3 Was sich nicht abbilden lässt, wird benannt.** Gruppen,
Schlagworte, Notizen, Filterregeln, Risiko-Einstellungen,
`post_ingest_policy`, `ai_injection_check_enabled`, `sftp_server_path`
und die Art der Anmeldung haben in `ssh_config` kein Gegenstück. Sie
erscheinen als Kommentarzeilen über dem jeweiligen Block, beginnend mit
`# smart-ssh:`, und zusätzlich in einer Zusammenfassung im Kopf der
Datei. Diese Kommentare sind **Hinweis, kein Format**: Der Import liest
sie nicht zurück (§4.4).

**3.2.4** Der lokale Pseudo-Server (`is_local`, `local_server.rs:34`)
wird **nicht** exportiert; er ist kein SSH-Ziel.

**3.2.5 Keine bestehende Datei wird überschrieben.** Der Export schreibt
in eine vom Nutzer gewählte **neue** Datei. Existiert sie bereits, fragt
der Dateidialog des Systems; darüber hinaus schreibt Smart SSH nie in
`~/.ssh/config`. Nach dem Schreiben nennt die Meldung den Pfad und die
Zeile, mit der man die Datei einbindet: `Include <pfad>` am **Anfang**
von `~/.ssh/config` (OpenSSH: der erste Wert gewinnt).

**3.2.6** Die erzeugte Datei MUSS von `ssh` akzeptiert werden: `ssh -F
<datei> -G <host>` liefert Rückgabewert 0 und die erwarteten Werte
(§6.3.2).

**3.2.7** Namen, die in `ssh_config` nicht als `Host`-Alias taugen
(Leerzeichen, `#`, Anführungszeichen, `*`, `?`, leer), werden für den
Export in einen gültigen Alias überführt. Die Regel steht in §4.3; die
Abbildung erscheint in der Meldung, damit der Nutzer den neuen Namen
kennt.

### 3.3 Grenzen

Eine Datei aus fremder Hand darf Smart SSH nicht lahmlegen. Die Grenzen
gelten **über alle gelesenen Dateien zusammen**, nicht je Datei — sonst
umginge eine Kette von `Include`-Dateien jede einzelne (§9/E-4). Wird
eine überschritten, bricht der Import mit verständlicher Meldung ab und
legt nichts an.

| Grenze | Wert |
|---|---|
| Gelesene Bytes insgesamt | 8 MiB |
| Zeilen insgesamt | 100 000 |
| `Host`-Blöcke insgesamt | 2 000 |
| Länge eines einzelnen Werts | 4 096 Zeichen |
| `Include`-Tiefe | 3 (§3.1.4) |
| Anzahl eingebundener Dateien | unbegrenzt |

## 4. Design

### 4.1 Wo der Code liegt

Die Abbildung von geparsten Blöcken auf `Server`/`Group` ist **reine
Logik** und gehört nach `crates/core/src/profiles/ssh_config/` — ohne
Tauri, ohne Dateisystem. Sie nimmt eine Liste bereits gelesener Dateien
(Pfad + Inhalt) entgegen und gibt einen Importplan zurück.

**Das Lesen der Dateien, inklusive `Include`-Auflösung, macht
`app-shell`** — es ist der einzige Teil, der das Dateisystem anfasst, und
genau der Teil, der Grenzen und Schleifenerkennung durchsetzt. Damit
bleibt die Architekturregel gewahrt („`core` never depends on Tauri")
**und** die gesamte Abbildungslogik ist ohne Dateisystem testbar.

Die Abbildung erzeugt **keinen** Server, sondern einen **Plan**: eine
Liste beabsichtigter Profile und Gruppen mit Herkunft, Konflikten und
nicht übernommenen Direktiven. Genau dieser Plan ist die Vorschau
(3.1.7), und genau er wird bestätigt ausgeführt. Damit können Vorschau
und Ergebnis nicht auseinanderlaufen — sie sind dasselbe Objekt.

### 4.2 Parser: `ssh2-config`

Entschieden (§9/E-1): Wir nehmen die Kiste `ssh2-config` (MIT, 0.7.2)
statt einen eigenen Parser zu schreiben. Sie liest **und** schreibt
(`to_string()`), wurde ausdrücklich für reine Rust-SSH-Stacks wie russh
gebaut, hat sechs kleine Abhängigkeiten (`bitflags`, `dirs`, `glob`,
`log`, `thiserror`, `wildmatch`), und ihr `HostParams` trägt neben
`host_name`, `user`, `port`, `proxy_jump` und `identity_file` auch
`ignored_fields` und `unsupported_fields` — womit sie §3.1.5 von selbst
erfüllt.

**Vor Schritt 1 wird gemessen, nicht angenommen** (§7.0): wie sie
`Include`, `Match` und einen Platzhalterblock behandelt. Fällt das
schlecht aus, schreiben wir den Parser doch selbst; die Entscheidung
kostet dann eine Viertelstunde und nicht einen Coder-Lauf. Ergebnis der
Messung gehört als Klarstellung in §9.

Die Grenzen aus §3.3 setzen wir in **jedem** Fall selbst davor, bevor
Inhalt an die Kiste geht — sie ist dafür nicht gebaut.

`Match` wird in dieser Spec **nicht** ausgewertet: Es hängt von Laufzeit
und Umgebung ab (`exec`, `originalhost`, `user`), also von Dingen, die
zum Importzeitpunkt niemand kennt. `Match`-Blöcke werden als nicht
übernommen gemeldet (3.1.5).

### 4.3 Namen

Import: Der `Host`-Alias wird `name`. Ist er leer oder schon vergeben,
greift 3.1.8.

Export: Ein Alias darf keine Leerzeichen, kein `#`, keine
Anführungszeichen und keine Platzhalter enthalten. Regel: Zeichen
außerhalb `[A-Za-z0-9._-]` werden zu `-`, Mehrfach-`-` zusammengezogen,
Ränder beschnitten; ist das Ergebnis leer, wird `server-<n>` benutzt;
bei Kollision wird `-2`, `-3` … angehängt. Die Abbildung wird gemeldet
(3.2.7).

### 4.4 Warum der Import die eigenen Kommentare nicht zurückliest

Man könnte Gruppen und Schlagworte in `# smart-ssh:`-Kommentare
schreiben und beim Import wieder herausziehen. Das wäre ein
**verstecktes Eigenformat in fremder Kleidung**: Eine Datei sähe aus wie
eine `ssh_config`, trüge aber Bedeutung, die nur wir kennen — und ein
Nutzer, der sie von Hand bearbeitet, zerstörte sie unbemerkt. Das eigene
Austauschformat ist BL-0081 und darf sich als solches zu erkennen geben.
Verworfen.

**Folge, ehrlich benannt:** Der Rundlauf (§6.3.3) erhält Name, Adresse,
Port, Benutzer, Jump-Host-Kette und den Schlüsselpfad — **nicht**
Gruppe, Schlagworte und Notizen. Die Meldung nach dem Export sagt das
(3.2.3).

### 4.5 Gruppen kommen nur aus Dateien, Platzhalter immer aus Schlagworten

Entschieden (§9/E-3). `ssh_config` kennt weder Gruppen noch Schlagworte;
beides müssen wir ableiten, und ein Server kann bei uns nur in **einer**
Gruppe liegen. Zwei Kandidaten für diese eine Gruppe standen zur Wahl:
die Herkunftsdatei und der Platzhalterblock.

**Die Datei gewinnt**, weil sie eindeutig ist: Jede Zeile stammt aus
genau einer Datei. Es entsteht:

```
<Dateiname der gewählten Datei>        ← Gruppe des Laufs
├── team.conf                          ← aus Include, Tiefe 1
│   └── kunden.conf                    ← aus Include, Tiefe 2
└── prod.conf
```

Jeder Server liegt in der Gruppe seiner Herkunftsdatei. Die
Verschachtelung nutzt `Group::parent_id` (`types.rs:35`); die Kette ist
über `group_chain` (`store.rs:107`) lesbar.

**Der Platzhalterblock wird ausnahmslos ein Schlagwort** (3.1.3) — es
gibt keinen Fall, in dem aus einem Platzhalter eine Gruppe wird, und
keinen, in dem eine Gruppe aus etwas anderem als einer Datei entsteht.
Das löst das
Problem, an dem die Gruppenlösung scheitert: Muster überlappen. Ein Host
passt auf `*.prod.de` **und** auf `*`; als Gruppe müsste der Import
raten, welche gewinnt, als Schlagwort bekommt der Server schlicht beide.
Keine Raterei, kein Informationsverlust.

Erzeugt der Import eine Gruppe, deren Name schon existiert, wird eine
**neue** Gruppe mit angehängter Nummer angelegt statt in die bestehende
hineinzuschreiben — sonst vermischen sich zwei Importe unbemerkt.
Wiederholbarkeit (3.1.10) bleibt gewahrt, weil bei einem zweiten Import
derselben Datei ohnehin kein Server übrig bleibt und die Gruppe dann
gar nicht erst angelegt wird.

### 4.6 Das Feld `identity_file`

Ein neues Feld `identity_file: Option<String>` am `Server`, gefüllt vom
Import, gelesen vom Export, angezeigt in der Oberfläche — **nicht
benutzt zur Anmeldung** (§1.2, §3.1.9). Wanderung
`0015_server_identity_file.sql` nach dem Muster von
`0014_server_sftp_server_path.sql`.

Der Pfad wird als Zeichenkette gespeichert, **unverändert**: kein
`realpath`, keine Existenzprüfung, kein Auflösen von `~`. `ssh` löst
`~` selbst auf, und jede Auflösung unsererseits wäre eine Aussage
darüber, welche Dateien auf dem Rechner liegen (§5.4).

Ein Feld, das aussieht, als täte es etwas, und es nicht tut, ist
schlimmer als keins — deshalb ist der Hinweis in der Oberfläche
(3.1.9) Teil der Anforderung und nicht Kosmetik.

**Was Spec 0076 daraus macht** (§9/E-5): einen Dateidialog beim Anlegen
eines Servers und eine ausdrückliche Nachlese für importierte Server.
Der Schlüssel wird dabei **einmal** gelesen und in den Schlüsselbund
gelegt; von da an benutzt Smart SSH die Fassung aus dem Schlüsselbund,
genau wie bei einem eingefügten Schlüssel heute. Die Datei wird nicht
bei jedem Verbinden erneut geöffnet. `identity_file` behält damit seine
Rolle: Es merkt sich, **woher** der Schlüssel kam, und füttert den
Export.

## 5. Sicherheits-Invarianten

Berührt sind **Credential-Handling**, mittelbar über die erzeugten
Profile **Ausführungspfad** und **Filter-/Risiko-Bewertung**, und neu:
**Dateizugriff auf Pfade, die aus einer fremden Datei stammen**. Daher
`Review-Priorität: ERHÖHT`.

**5.1 Kein Geheimnis wird gelesen oder geschrieben.** Der Import öffnet
ausschließlich `ssh_config`-Dateien — die gewählte und die über
`Include` erreichten. **Nie** eine Datei, auf die `IdentityFile` zeigt,
und nie den Schlüsselbund zum Schreiben von Schlüsselmaterial. Der
Export schreibt keinen Wert, der aus dem Schlüsselbund stammt.
Nachgewiesen durch §6.4.1 und §6.4.2.

**5.2 Eine fremde Datei ändert keine Sicherheitseinstellung.**
`post_ingest_policy` und `ai_injection_check_enabled` kommen
ausschließlich aus den Produktvorgaben (3.1.12). Eine `ssh_config` darf
nicht dazu führen, dass ein Profil mit schwächerer Prüfung entsteht.
Nachgewiesen durch §6.4.3.

**5.3 Kein unbemerkter Jump-Host.** Jeder `jump_host`, der beim Import
entsteht, steht in der Vorschau. Eine Jump-Host-**Schleife** wird beim
Import abgelehnt, nicht erst beim Verbinden — heute prüft das nur
`jump_host.rs:27-29`, also nachdem ein Nutzer sich schon verbinden
wollte. Der Import bringt die Prüfung nach vorn (§1.3). Nachgewiesen
durch §6.4.4.

**5.4 Gelesene Dateiinhalte bleiben auf dem Gerät — das ist die
Invariante, die `Include` erst zulässig macht.**

Mit der Entscheidung E-4 bestimmt eine fremde Datei, welche weiteren
Dateien wir öffnen, ohne Einschränkung der Pfade. Eine untergeschobene
`ssh_config` kann also `Include /etc/passwd` enthalten, und wir lesen
sie. Was das **nicht** ist: ein Weg, Daten abfließen zu lassen — der
Nutzer kann diese Dateien ohnehin selbst lesen. Was es **wäre**, wenn
wir nicht aufpassen: ein Weg, fremden Dateiinhalt irgendwohin zu
befördern. Daraus folgen drei bindende Regeln:

1. Der Inhalt gelesener Dateien, die Liste nicht übernommener
   Direktiven (3.1.5) und die Dateiliste (3.1.4) **verlassen das Gerät
   nicht**: nicht in eine Fehlermeldung nach außen, nicht in
   Telemetrie, nicht in einen KI-Prompt, nicht in die Zwischenablage
   ohne ausdrückliche Nutzeraktion.
2. Nichts aus einer gelesenen Datei wird **ausgeführt**, interpretiert
   oder als Kommando behandelt.
3. Die Vorschau nennt **jede** gelesene Datei mit vollem Pfad (3.1.4).
   Ein Nutzer, dem eine Datei untergeschoben wurde, sieht vor dem
   Anlegen, was sie aufgemacht hat — das ist der Ersatz für die
   Pfadprüfung, auf die wir verzichten.

Nachgewiesen durch §6.4.9.

**5.5 Kein Pfad zu einer Schlüsseldatei wird aufgelöst.** Ein
`IdentityFile`-Wert ist für uns eine Zeichenkette (§4.6): kein
`realpath`, keine Existenzprüfung, kein Lesen. Damit gibt es weder
Pfaddurchquerung noch ein Orakel darüber, welche Schlüssel auf dem
Rechner liegen.

**5.6 Begrenzte Aufnahme.** §3.3, über alle Dateien zusammen, plus
Tiefenbegrenzung und Schleifenerkennung (3.1.4). Eine überlange, tief
verschachtelte oder zyklische Konfiguration führt zu einer Meldung,
nicht zu Speicherdruck oder einer Endlosschleife. Nachgewiesen durch
§6.4.5.

**5.7 Der lokale Pseudo-Server bleibt außen vor** — nicht exportiert
(3.2.4), nicht als Jump-Host importierbar (3.1.6). Keine
Sonderbehandlung in der Sicherheitslogik, nur an der Grenze: genau so,
wie es die Architekturregel verlangt.

## 6. Tests

### 6.1 Abbildung (in `core`, ohne Dateisystem)

1. Ein Block mit `HostName`, `Port`, `User` → ein Plan-Eintrag mit genau
   diesen Werten. **Scheitert bei kaputter Implementierung**, weil ein
   vertauschtes oder verschlucktes Feld sofort abweicht.
2. Block ohne `HostName` → `host` ist der Alias.
3. Block ohne `Port` → 22.
4. Groß-/Kleinschreibung der Direktiven (`hostname`, `HOSTNAME`) wird
   erkannt; Werte bleiben unverändert.
5. Anführungszeichen und `=` als Trenner (`Port=2222`,
   `User "max mustermann"`) werden nach OpenSSH-Regeln gelesen.
6. `Host web1 web2` (mehrere Aliase) → zwei Plan-Einträge mit denselben
   Werten.
7. Platzhalterblock `Host *.prod.de` mit `User deploy`, danach `Host
   web1.prod.de` ohne `User` → `web1.prod.de` bekommt `deploy` **und**
   das Schlagwort `*.prod.de`, und es entsteht **kein** Profil namens
   `*.prod.de` (3.1.3).
8. Derselbe Fall, aber `Host web1.prod.de` setzt `User root` **vor** dem
   Platzhalterblock → `root` bleibt stehen („der erste gewinnt").
9. Ein Host passt auf `*.prod.de` und auf `*.de` → **zwei** Schlagworte.
10. `Host *` erzeugt kein Schlagwort (3.1.3).
11. Zwei `IdentityFile`-Zeilen in einem Block → der erste Wert steht im
    Feld, der zweite in der Liste der nicht übernommenen Direktiven
    (3.1.9).
12. Kommentare und Leerzeilen ändern nichts.

### 6.2 Dateien, `Include`, Gruppen (in `app-shell`)

1. Datei ohne `Include` → eine Gruppe, benannt nach der Datei, alle
   Server darin.
2. Datei bindet `team.conf` ein, das `kunden.conf` einbindet → drei
   Gruppen in der erwarteten Eltern-Kind-Kette, jeder Server in der
   Gruppe seiner Herkunftsdatei (§4.5).
3. `Include conf.d/*.conf` mit drei Treffern → drei Untergruppen.
4. Relativer `Include`-Pfad wird relativ zum Verzeichnis der
   **einbindenden** Datei aufgelöst, nicht zum Arbeitsverzeichnis
   (3.1.4).
5. Absoluter Pfad und ein Pfad mit `..` werden gefolgt (E-4).
6. `Include` auf Tiefe 3 wird nicht mehr gefolgt und erscheint als nicht
   übernommen (3.1.4).
7. Eine nicht lesbare `Include`-Datei lässt den Import weiterlaufen und
   wird gemeldet (3.1.4).
8. Die Vorschau listet jede gelesene Datei mit vollem Pfad, in
   Lesereihenfolge.
9. `ProxyJump` auf einen Host, der in einer **anderen** eingebundenen
   Datei steht, wird aufgelöst (3.1.6).

### 6.3 Vorschau, Konflikte, Export, Rundlauf

1. Zwölf Hosts, Platzhalterblock, ein `ProxyJump` → Vorschau und
   Ergebnis stimmen Feld für Feld überein (BL-0216, Akzeptanz).
2. **`ssh -F <datei> -G <host>`** gibt 0 zurück und meldet `hostname`,
   `port`, `user`, `identityfile` wie erwartet (3.2.6). `-G` statt einer
   echten Verbindung: prüft die Datei, braucht kein Netz.
3. **Rundlauf:** Export → Import in einen leeren Bestand → Name,
   Adresse, Port, Benutzer, Jump-Host-Kette und `identity_file` stimmen
   mit dem Original überein (BL-0218, Akzeptanz). Gruppe, Schlagworte
   und Notizen werden ausdrücklich **nicht** verglichen (§4.4).
4. Zweiter Import derselben Datei → **null** neue Profile und **keine**
   neue Gruppe (3.1.10).
5. Namenskonflikt → Vorgabe „überspringen"; das bestehende Profil ist
   danach unverändert (Feldvergleich vorher/nachher).
6. Unbekannte Direktive (`Compression yes`) erscheint in der Liste der
   nicht übernommenen Direktiven **mit Datei und Zeilennummer** (3.1.5).
7. `Match`-Block erscheint als nicht übernommen (§4.2).
8. Abbruch in der Vorschau → Serverliste und Gruppenliste unverändert.
9. Fehler mitten im Anlegen → kein Profil **und keine Gruppe** dieses
   Laufs bleibt übrig (3.1.11).
10. Der lokale Pseudo-Server kommt in der exportierten Datei nicht vor
    (3.2.4).
11. Ein Server mit Leerzeichen und `#` im Namen erzeugt einen gültigen
    Alias; die Meldung nennt die Umbenennung (3.2.7).
12. Zwei Server, deren Namen auf denselben Alias abbilden, erhalten
    unterschiedliche Aliase (4.3).

### 6.4 Adversariale Fälle (Pflicht bei ERHÖHT)

1. **Keine Schlüsseldatei wird gelesen.** Eine Konfiguration zeigt mit
   `IdentityFile` auf eine im Test angelegte Datei mit erkennbarem
   Inhalt. Nach dem Import: Der **Pfad** steht im Feld, der **Inhalt**
   kommt nirgends vor — nicht im Plan, nicht im Profil, nicht in der
   Datenbank, nicht im Log. Der Test scheitert, sobald jemand
   „hilfsbereit" den Schlüssel einliest.
2. **Kein Geheimnis im Export.** Ein Server mit Passwort und ein Server
   mit hinterlegtem Schlüssel werden exportiert; die erzeugte Datei wird
   gegen die Leak-Muster und gegen die im Test gesetzten Werte geprüft —
   kein Treffer (3.2.2).
3. **Eine fremde Datei schwächt keine Einstellung.** Eine Konfiguration
   mit `PostIngestPolicy allow`, `AiInjectionCheck no` und denselben
   Werten als `# smart-ssh:`-Kommentar. Ergebnis: beide Felder stehen
   auf den Produktvorgaben (5.2). Der Test scheitert, sobald der Import
   irgendeinen Weg öffnet, sie zu setzen.
4. **Jump-Host-Schleife.** `a` springt über `b`, `b` über `a`. Der
   Import legt die Schleife **nicht** an und sagt warum (5.3). Zweiter
   Fall: eine Kette über drei Hosts zurück auf den ersten. Dritter Fall:
   die Schleife verteilt über zwei `Include`-Dateien.
5. **Aufblähung und Schleifen in `Include`.**
   (a) Gesamtgröße über 8 MiB, verteilt auf viele kleine Dateien —
   greift die **Gesamt**grenze, nicht die je Datei (§3.3);
   (b) 3 000 `Host`-Blöcke über mehrere Dateien;
   (c) ein Wert mit 100 000 Zeichen;
   (d) eine Datei, die sich **selbst** einbindet;
   (e) `a.conf` → `b.conf` → `a.conf`;
   (f) `Include *.conf` in einem Verzeichnis, das die einbindende Datei
   selbst enthält.
   Jeder Fall: verständliche Meldung, nichts angelegt, kein Hänger
   (5.6).
6. **Bösartige Aliase.** `Host ../../etc/passwd`, `Host $(whoami)`,
   `Host a b` mit Steuerzeichen, ein Alias mit `\n` im Wert. Ergebnis:
   entweder abgelehnt oder als harmloser Name übernommen — in keinem
   Fall entsteht ein Pfad, ein Kommando oder eine zweite Zeile in der
   exportierten Datei.
7. **Unsichtbare Randzeichen.** Ein `HostName` mit BOM und
   Zero-Width-Space an den Rändern. Verhalten muss zu Spec 0073 passen
   (derselbe geteilte Trim), damit nicht zwei Wege mit zwei Ergebnissen
   entstehen.
8. **Der lokale Pseudo-Server als Jump-Host.** Eine Konfiguration, deren
   `ProxyJump` auf den Namen des lokalen Servers zeigt → abgelehnt mit
   `SERVER_JUMP_HOST_LOCAL` (5.7).
9. **Gelesener Fremdinhalt bleibt hier.** Eine Konfiguration mit
   `Include` auf eine Datei, die kein `ssh_config` ist und eine
   erkennbare Zeichenfolge enthält. Der Test prüft, dass diese
   Zeichenfolge zwar in der Vorschau erscheinen **darf** (der Nutzer
   soll sehen, was aufgemacht wurde), aber in **keinem** ausgehenden
   Weg auftaucht: kein KI-Prompt, keine Telemetrie, keine
   Fehlermeldung nach außen, kein Log außerhalb des Geräts (5.4).
   Zugleich: der volle Pfad der Datei steht in der Vorschau.

## 7. Umsetzungsreihenfolge

Jeder Schritt ist für sich committbar und lässt das Gate grün.

**0. Messen, bevor die Abhängigkeit steht** *(vor Schritt 1, ~15 min)*.
Kleines Testprogramm gegen `ssh2-config` 0.7.2: Wie behandelt sie
`Include`, `Match` und einen Platzhalterblock? Liefert sie
`unsupported_fields` mit Zeilennummern? Ergebnis als Klarstellung in §9.
Fällt es schlecht aus, wird Schritt 1 ein eigener Parser — dieselben
Tests, andere Innerei.

1. **Abhängigkeit und Abbildung** in
   `crates/core/src/profiles/ssh_config/`: Blöcke → Importplan, inklusive
   Platzhalter-Vorgaben und -Schlagworten (3.1.3), `ProxyJump`-Auflösung
   (3.1.6), Konflikten (3.1.8), Zyklusprüfung (5.3). Tests §6.1,
   §6.4.3, §6.4.4, §6.4.6, §6.4.7. *(opus — hier entsteht die Logik,
   gegen die §6.4 fährt)*
2. **Dateizugriff in `app-shell`**: `Include`-Auflösung mit Tiefe,
   Schleifenerkennung und Platzhalterpfaden (3.1.4), Gesamtgrenzen
   (§3.3), Gruppenbaum (§4.5). Tests §6.2, §6.4.5, §6.4.9. *(opus —
   hier liegt die neue Angriffsfläche)*
3. **Persistenz**: Wanderung `0015_server_identity_file.sql` nach dem
   Muster von `0014`; Feld durch `Server`, `ServerInput`, Speicher und
   DTOs durchreichen. *(opus — Wanderung)*
4. **Kommandos**: `preview_ssh_config_import`,
   `apply_ssh_config_import`, `export_ssh_config`; Rollback für Profile
   **und** Gruppen (3.1.11); `is_local` ausschließen. Tests §6.3.8–10,
   §6.4.1, §6.4.2, §6.4.8. *(opus — Credential-Nähe)*
5. **Schreiber** (Export) inklusive Alias-Regel (4.3) und Kommentaren
   (3.2.3). Tests §6.3.1–3, §6.3.11–12. *(sonnet)*
6. **Frontend**: Vorschau-Dialog mit Abwahl je Eintrag,
   Konfliktanzeige, Dateiliste, Gruppenbaum, Liste der nicht
   übernommenen Direktiven; Hinweis am `identity_file`-Feld (3.1.9);
   Export-Dialog mit der `Include`-Zeile. i18n vollständig. *(sonnet)*
7. **`CHANGELOG.md`** unter `[Unreleased]`, Nutzersicht. *(sonnet)*

Der Rundlauftest (§6.3.3) läuft ab Schritt 5 und ist die Abnahme für
beide Items.

## 8. Offene Punkte

Keine. Die vier Punkte, die diese Spec zur Entscheidung vorgelegt hat,
sind in §9 als E-1 bis E-4 entschieden und in den Text eingearbeitet.

Ein Punkt bleibt **bewusst** außerhalb und ist kein offener Punkt,
sondern eine benannte Folge: Der Rundlauf ist **nicht verlustfrei** —
Gruppen, Schlagworte und Notizen gehen nur als Kommentar mit und werden
beim Import nicht zurückgelesen (§4.4). Der verlustfreie Weg ist
BL-0081.

## 9. Klarstellungen

**2026-09-23 · E-1 · Parser.** Stefan: `ssh2-config` (MIT, 0.7.2) wird
als Abhängigkeit aufgenommen, statt einen eigenen Parser zu schreiben —
mit der Bedingung, dass Schritt 7.0 ihr Verhalten bei `Include`, `Match`
und Platzhalterblöcken **misst**, bevor die Abhängigkeit steht.
Eingearbeitet in §4.2 und §7.0.

**2026-09-23 · E-2 · `IdentityFile`.** Stefan: Der Pfad wird in einem
neuen Feld `identity_file` gemerkt und exportiert, aber von dieser Spec
**nicht** zur Anmeldung benutzt; die dateibasierte Anmeldung wird
**unmittelbar danach als eigene Spec 0076** geschrieben, nicht als
Backlog-Item für später. Eingearbeitet in §1.2, §2 (Nicht-Ziel 4),
§3.1.9, §4.6, §5.5 und §7.3.

**2026-09-23 · E-3 · Gruppen und Schlagworte.** Stefan: eine Gruppe je
Import, darin je eine Untergruppe pro eingebundener Datei; die
Zugehörigkeit zu einem Platzhalterblock wird ein **Schlagwort**, nicht
eine Gruppe. Damit ist die Zuordnung eindeutig (jede Zeile stammt aus
genau einer Datei) und überlappende Muster lösen sich von selbst, statt
dass der Import raten muss. Eingearbeitet in §3.1.3, §4.5 und §6.1.7–10.

**2026-09-23 · E-4 · `Include`.** Stefan: `Include` wird **gefolgt**.
Pfade werden nicht eingeschränkt (absolut, `~` und `..` zulässig), Tiefe
höchstens 3, keine Schleifen, **keine Anzahlgrenze** für Dateien, und
jede eingebundene Datei wird ein Ordner in Smart SSH. Eingearbeitet in
§3.1.4, §4.5 und §5.4.

**2026-09-23 · E-5 · Zuschnitt von Spec 0076.** Stefan: Zusammen mit der
Unterstützung für Schlüsseldateien soll auch das **Anlegen** eines Servers
mit Schlüsseldatei möglich sein; die Datei wird **eingelesen und im
Schlüsselbund gespeichert**, und danach wird die Fassung aus dem
Schlüsselbund benutzt. Damit ist 0076 kein neuer Anmeldeweg, sondern ein
Dateidialog vor dem bestehenden `key_content`-Pfad — die Anmeldung selbst
bleibt unverändert (`auth.rs:69`). Für diese Spec ändert sich nichts außer
dem Ausblick in §4.6; die Grenze bleibt: **Spec 0075 öffnet keine
Schlüsseldatei**, auch nicht die, deren Pfad sie gerade importiert hat.
Das Einlesen ist in 0076 immer eine eigene, sichtbare Handlung des
Nutzers — beim Anlegen über den Dateidialog, für importierte Server über
eine Nachlese, die je Server zeigt, was gelesen würde.

*Anmerkung des Architekten zur Grenze:* „Unbegrenzt viele Dateien"
bezieht sich auf die **Anzahl**. Die Gesamtgrenzen aus §3.3 (Bytes,
Zeilen, `Host`-Blöcke) gelten weiterhin — aber ausdrücklich **über alle
Dateien zusammen** statt je Datei, weil eine Grenze je Datei durch eine
Kette von `Include`-Dateien umgangen würde. Ohne irgendeine
Gesamtgrenze wäre §5.6 nicht haltbar. Widerspricht das der Absicht,
bitte in §9 korrigieren.
