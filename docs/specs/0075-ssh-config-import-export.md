# Spec 0075 — Server-Profile aus `ssh_config` importieren und dorthin exportieren

Status: Vorschlag (Architekt) · Backlog: BL-0216, BL-0218 · Gate: release-1.0/D
Repo: **öffentlich** `smart-ssh` — `crates/core/src/profiles/` (Abbildung),
`crates/app-shell/src/` (Kommandos, Dateizugriff, DTOs), Frontend
(Vorschau-Dialog)
**Setzt Spec 0076 voraus** (Anmeldeart `AuthMethod::IdentityFile`); deren
Umsetzung geht dieser voraus, s. §7.
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
$ grep -rlnE "ssh_config|\.ssh/config|SshConfig" --include="*.rs" .
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

Entschieden (§9, E-2): **Spec 0076 führt sie ein** — eine fünfte Variante
`AuthMethod::IdentityFile { path, passphrase_ref }`, in der Oberfläche
wählbar wie jede andere, plus einen Knopf, der eine Schlüsseldatei in einen
gespeicherten Schlüssel überführt. Diese Spec **benutzt** sie und fragt
beim Import, welchen Weg der Nutzer will (§3.1.9).

Dass das ohne Wanderung geht, ist gemessen: `auth_method` liegt als **JSON**
in einer Textspalte (`crates/persistence-sqlite/migrations/0001_initial.sql:31`
— „JSON-serialisiertes AuthMethod-Enum"), serialisiert über
`auth_method_to_json`/`_from_json` (`crates/persistence-sqlite/src/mapping.rs:9-17`).
Eine zusätzliche Variante braucht deshalb **keine Schema-Änderung** — und
kein eigenes Feld neben der Anmeldeart, in dem derselbe Pfad ein zweites
Mal stünde.

### 1.3 Was beim Anlegen eines Servers **nicht** geprüft wird

Gemessen in `crates/app-shell/src/servers.rs:33-80` und
`crates/app-shell/src/dto.rs:492-521`:

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
- `servers.jump_host_id` ist ein **Fremdschlüssel** auf `servers(id)`
  (`crates/persistence-sqlite/migrations/0001_initial.sql:33`). Ein Profil
  mit `jump_host` lässt sich also erst anlegen, wenn sein Ziel schon
  existiert — das bestimmt die Einfügereihenfolge beim Import (3.1.11).

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
4. **Keine neue Anmeldeart.** Die Anmeldeart `AuthMethod::IdentityFile`,
   ihre Oberfläche und der Überführungsknopf sind **Spec 0076**. Diese
   Spec benutzt sie nur; §7 macht 0076 zur harten Vorbedingung.
5. **Keine Schlüsseldatei wird ungefragt geöffnet.** Der Import liest eine
   Schlüsseldatei **nur**, wenn der Nutzer in der Vorschau ausdrücklich
   Weg (b) gewählt hat, und nur die Dateien, die die Vorschau vorher
   namentlich genannt hat (§3.1.9, §5.1).
6. **Kein Geheimnis im Export.** Der Export schreibt nie ein Passwort,
   eine Passphrase oder Schlüsselmaterial — nur Pfade.
7. **Keine Verwaltung der echten `~/.ssh/config`.** Wir schreiben nie in
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
| `Host <alias>` | `name` | **je Alias** ein Profil; `Host web1 web2` ergibt zwei |
| `HostName` | `host` | fehlt sie, gilt der Alias als Adresse (`ssh`-Verhalten) |
| `Port` | `port` | fehlt sie: 22 |
| `User` | `username` | fehlt sie: leer |
| `ProxyJump` | `jump_host` | §3.1.6 |
| `IdentityFile` | Anmeldeart | der Nutzer wählt, §3.1.9 |
| `Include` | Untergruppe | §3.1.4 |

**Randzeichen.** Von jedem übernommenen Wert werden am **Rand** dieselben
unsichtbaren Zeichen entfernt, die Spec 0073 für Zugangsdaten festlegt.
Die Liste existiert bereits als `INVISIBLE_CREDENTIAL_EDGE_CHARS`
(`crates/core/src/profiles/credentials.rs:17`, re-exportiert in
`profiles/mod.rs:17-18`) — sie wird **benutzt**, nicht abgeschrieben,
sonst steht dieselbe Tatsache an zwei Stellen. Nicht weil es
Zugangsdaten wären — 0073 gilt ausdrücklich nur für die —, sondern damit
ein aus einer Webseite kopierter `HostName` nicht an einem unsichtbaren
Zeichen scheitert. Die **Zeichenliste** wird geteilt, nicht der
Geltungsbereich. Zeichen **innerhalb** eines Werts bleiben unangetastet.
Getrimmt wird **nach** dem Auflösen der Anführungszeichen: `User " max"`
ergibt `max`.

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

**3.1.4a Eine Datei, die keine `ssh_config` ist, wird übersprungen —
nicht Zeile für Zeile gemeldet.** Erkannt an einem der drei Merkmale:
Sie enthält ein NUL-Byte, sie ist kein gültiges UTF-8, oder sie enthält
**keine einzige** erkannte Direktive. Die Meldung lautet dann
`<pfad>: keine ssh_config, übersprungen` — **ohne Inhalt**. Das gilt für
eingebundene Dateien; für die **gewählte** Datei greift stattdessen
§3.1.13 (Abbruch mit Meldung).

Das ist die Stelle, an der der Parser den Anzeige-Kanal aus §5.4
zumacht, statt dass eine Regel ihn später auffangen müsste.

**3.1.5 Nicht übernommene Direktiven werden gemeldet — mit Namen, nicht
mit Inhalt.** Je Block und Direktive, mit **Datei und Zeilennummer**.
Gemeldet wird der **Name** der Direktive, also das erste Wort der Zeile,
**nie ihr Wert und nie die ganze Zeile**. Sieht das erste Wort nicht wie
ein Direktivenname aus (nicht `[A-Za-z][A-Za-z0-9-]{0,31}`), lautet die
Meldung `unlesbare Zeile` mit der Zeilennummer und sonst nichts.

Stillschweigend verschlucken bleibt ausgeschlossen (BL-0216,
Akzeptanz) — der Nutzer erfährt **dass** und **wo** etwas nicht
übernommen wurde, nur eben nicht den Inhalt.

**3.1.6 `ProxyJump`** bildet auf `jump_host: Option<ServerId>` ab:

- Ein einzelner Hop, dessen Ziel im selben Import (über alle Dateien
  hinweg) oder bereits im Bestand vorkommt → `jump_host` zeigt darauf.
- **Mehrere Hops** (`ProxyJump a,b,c`): Unser Modell kettet über je ein
  `jump_host` **pro Server**, kann eine Kette also nur darstellen, indem
  es `jump_host` **an den Zwischenstationen** setzt.

  **Die Richtung gehört hingeschrieben, sonst rät sie jemand.** In
  OpenSSH ist `a` der **zuerst** kontaktierte Hop; bei uns zeigt
  `jump_host` auf den Hop, **über den** ein Server erreicht wird. Für
  `Host x` mit `ProxyJump a,b,c` entstehen also genau diese Kanten:

  ```
  x.jump_host = c
  c.jump_host = b
  b.jump_host = a
  a.jump_host = (nicht gesetzt)
  ```

  „Zwischenstation" meint im Folgenden `a`, `b` und `c` — alle in der
  `ProxyJump`-Zeile genannten Hosts, auch `a`, an dem am Ende kein
  `jump_host` steht. Daraus folgen zwei harte Bedingungen, ohne die der
  Import bestehende Profile verändern oder Unmögliches versuchen würde:

  1. **Alle Zwischenstationen entstehen in diesem Import neu.** Liegt
     auch nur eine schon im Bestand, wird die Kette **nicht** angelegt —
     sie zu bauen hieße, ein fremdes Profil zu ändern, und das verbietet
     3.1.8.
  2. **Für das `jump_host` einer Zwischenstation gibt es genau eine
     Quelle.** `jump_host` ist einwertig (`types.rs:62`,
     `Option<ServerId>`). Eine zweite Quelle entsteht auf zwei Wegen, und
     beide zählen: wenn `b` in **zwei Ketten** mit verschiedenen
     Vorgängern vorkommt, **oder** wenn `b` in seinem **eigenen**
     `Host`-Block ein `ProxyJump` trägt (`Host x` mit `ProxyJump a,b`
     und zugleich `Host b` mit `ProxyJump q`). In beiden Fällen gilt
     dieselbe Rechtsfolge.

  Ist eine der beiden Bedingungen verletzt, wird `ProxyJump` bei **allen**
  beteiligten Einträgen als nicht übernommen gemeldet (3.1.5), mit der
  Begründung — nicht gekürzt und nicht teilweise angelegt.
- **Schleifen** (`a` über `b`, `b` über `a`, oder länger, auch über
  mehrere Dateien): Der Import legt sie **nicht** an und meldet
  `ProxyJump` bei **allen** beteiligten Einträgen als nicht übernommen.
  Heute fällt eine Schleife erst beim Verbinden auf
  (`jump_host.rs:27-29`); der Import zieht die Prüfung nach vorn (§5.3).
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

**3.1.9 `IdentityFile` — der Nutzer entscheidet, was damit geschieht.**
Die Vorschau bietet drei Wege an, umstellbar für den ganzen Import und
einzeln je Eintrag. Voreingestellt ist (a), weil nur dieser Weg keine
einzige zusätzliche Datei öffnet:

**(a) Als Schlüsseldatei übernehmen** *(Vorgabe)*. Der Server bekommt
`AuthMethod::IdentityFile { path, passphrase_ref: None }` (Spec 0076).
Der Pfad wird **unverändert** übernommen — kein `realpath`, keine
Existenzprüfung, kein Auflösen von `~` (§4.6). **Die Datei bleibt
ungeöffnet.** Später überführt der Nutzer sie mit einem Knopfdruck in
einen gespeicherten Schlüssel, wenn er will (Spec 0076).

  **Ein relativer Pfad oder ein Pfad in `~user`-Form wird übernommen,
  aber sichtbar markiert.** `ssh_config` erlaubt beides; Spec 0076 (A-3)
  lehnt beides beim **Verbinden** ab. Ohne Hinweis entstünden hier
  stillschweigend Server, die sich nie anmelden können. Die Vorschau
  kennzeichnet solche Einträge deshalb als „beim Verbinden nicht
  benutzbar, absoluter Pfad nötig".

**(b) Schlüssel einlesen und im Schlüsselbund ablegen.** Beim
**Bestätigen** des Imports — nicht in der Vorschau — wird jede betroffene
Datei **einmal** gelesen und ihr Inhalt in den Schlüsselbund gelegt; der
Server bekommt `AuthMethod::PrivateKey`. Dabei gilt:

  - Die Vorschau nennt **vorher** jede Datei, die dabei geöffnet würde,
    mit vollem Pfad, und wie viele es insgesamt sind.
  - Eine Datei, die fehlt, nicht lesbar ist oder kein gültiger
    OpenSSH-Schlüssel ist, lässt den Import **nicht** scheitern: Dieser
    eine Server fällt auf Weg (a) zurück und die Meldung sagt, warum.
  - **Ein verschlüsselter Schlüssel wird verschlüsselt übernommen**
    (Spec 0076, C-4: der Dateiinhalt geht byte-gleich in den
    Schlüsselbund). **Nach einer Passphrase wird beim Import nicht
    gefragt.** Die Vorschau sagt bei solchen Einträgen, dass die
    Passphrase nachzutragen ist, bevor die erste Verbindung gelingt.
  - Der Dateiinhalt geht **ausschließlich** in den Schlüsselbund —
    nicht in den Plan, nicht in die Datenbank, nicht in ein Log
    (§5.1, §6.4.1).

**(c) Nicht übernehmen.** Der Server bekommt `AuthMethod::Agent`,
`IdentityFile` erscheint unter „nicht übernommen" (3.1.5).

Mehrere `IdentityFile`-Zeilen in einem Block: Der erste Wert gewinnt, die
weiteren werden als nicht übernommen gemeldet.

**3.1.10 Wiederholbarkeit.** Ein zweiter Import derselben Datei ohne
zwischenzeitliche Änderung legt **nichts** an (alles Konflikt, alles
Vorgabe „überspringen") — auch keine zweite Gruppe.

**3.1.11 Alles oder nichts je Bestätigung.** Schlägt das Anlegen mitten
in einem bestätigten Import fehl, bleibt kein halb angelegter Zustand:
Bereits erzeugte Profile **und Gruppen** dieses Laufs werden
zurückgenommen, und die Meldung nennt den fehlgeschlagenen Eintrag. Das
entspricht dem bestehenden Rollback-Verhalten in `servers.rs:44-54`.

**Reihenfolge:** `servers.jump_host_id` ist ein Fremdschlüssel auf
`servers(id)` (§1.4). Jump-Host-**Ziele** werden deshalb vor den
Profilen angelegt, die auf sie zeigen. `groups.parent_id` ist ebenfalls
ein Fremdschlüssel (`0001_initial.sql:16`), und §4.5 baut einen
mehrstufigen Baum — also: **Elterngruppen vor Untergruppen, Gruppen vor
ihren Servern, Jump-Host-Ziele vor ihren Nutzern.**

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
`ProxyJump <Alias des Jump-Hosts>` (wenn gesetzt) und `IdentityFile
<pfad>` — Letzteres genau dann, wenn die Anmeldeart des Servers
`AuthMethod::IdentityFile` ist (Spec 0076).

**`ProxyJump` nennt den Alias, nicht den Namen.** Wurde der Jump-Host
beim Export nach §4.3 umbenannt (Leerzeichen im Namen, Kollision), trägt
die `ProxyJump`-Zeile den **umbenannten** Alias — sonst zeigt sie auf
einen `Host`-Block, den es in der Datei nicht gibt, und §3.2.6
scheitert.

**3.2.2 Kein Geheimnis in der Datei.** Nie ein Passwort, nie eine
Passphrase, nie Schlüsselmaterial. Ein Server mit
`AuthMethod::Password` verliert beim Export seine Anmeldung — das ist
beabsichtigt und wird als Kommentar vermerkt (3.2.3).

**3.2.3 Was sich nicht abbilden lässt, wird benannt.** Gruppen,
Schlagworte, Notizen, Filterregeln, Risiko-Einstellungen,
`post_ingest_policy`, `ai_injection_check_enabled`, `sftp_server_path`
und die Art der Anmeldung haben in `ssh_config` kein Gegenstück. Das
trifft ausdrücklich auch den Server, dessen Schlüssel **im
Schlüsselbund** liegt (`AuthMethod::PrivateKey`): Für ihn gibt es keinen
Pfad, den wir schreiben könnten, also steht im Kommentar, dass sein
Schlüssel in der Datei fehlt und woher er stammt. Diese Angaben
erscheinen als Kommentarzeilen über dem jeweiligen Block, beginnend mit
`# smart-ssh:`, und zusätzlich in einer Zusammenfassung im Kopf der
Datei. Diese Kommentare sind **Hinweis, kein Format**: Der Import liest
sie nicht zurück (§4.4).

**3.2.4** Der lokale Pseudo-Server (`is_local`, `local_server.rs:34`)
wird **nicht** exportiert; er ist kein SSH-Ziel.

**3.2.5 Keine bestehende Datei wird überschrieben.** Der Export schreibt
in eine vom Nutzer gewählte **neue** Datei. Existiert sie bereits, fragt
der Dateidialog des Systems. **Unabhängig davon lehnt der Export
`~/.ssh/config` selbst als Ziel ab** — auch wenn der Nutzer sie im Dialog
auswählt und bestätigt. Es ist die Datei, deren Verlust am teuersten
wäre, und der Export hat keinen Grund, sie anzurühren (§2,
Nicht-Ziel 7). Nach dem Schreiben nennt die Meldung den Pfad und die
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
Tauri, ohne Dateisystem. Sie nimmt **drei** Eingaben entgegen und gibt
einen Importplan zurück: die Liste bereits gelesener Dateien (Pfad +
Inhalt), den **bestehenden Bestand** an Servern und Gruppen und die
**bestehenden Filterregeln**.

Die letzten beiden sind keine Zutat, sondern Voraussetzung: Ohne den
Bestand lassen sich weder Konflikte (3.1.8) noch `ProxyJump`-Ziele im
Bestand (3.1.6) bestimmen, ohne die Regeln nicht die Schlagwort-Treffer
(5.2a). Läge diese Arbeit stattdessen in `app-shell`, liefen Vorschau
und Plan wieder auseinander — genau das, was dieser Abschnitt
verhindert.

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
`ignored_fields` und `unsupported_fields` — womit sie die **Bausteine**
für §3.1.5 liefert. Ob sie dabei **Zeilennummern** mitgibt, die §3.1.5
verlangt, ist **nicht belegt** und genau eine der Fragen aus §7.0. Bis
dahin gilt: Bausteine ja, Anforderung erfüllt erst nach der Messung.

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

Import: Der `Host`-Alias wird `name`. Ist er **schon vergeben**, greift
3.1.8. Ist er **leer** — auch erst nach dem Randzeichen-Trim aus §3.1.2,
siehe §6.4.7 (c) —, wird **kein Profil angelegt**, sondern der Block als
nicht übernommen gemeldet (3.1.5). Beim Anlegen von Hand greift heute
kein Pflichtfeld-Check (§1.3); der Import bringt ihn mit, weil er sonst
namenlose Profile in Serie erzeugen könnte.

Export: Ein Alias darf keine Leerzeichen, kein `#`, keine
Anführungszeichen und keine Platzhalter enthalten. Regel: Zeichen
außerhalb `[A-Za-z0-9._-]` werden zu `-`, Mehrfach-`-` zusammengezogen,
Ränder beschnitten; ist das Ergebnis leer, wird `server-<n>` benutzt;
bei Kollision wird `-2`, `-3` … angehängt. Die Abbildung wird gemeldet
(3.2.7).

**Werte** brauchen eine eigene Regel, nicht nur Aliase: Ein `username`
mit Leerzeichen ist möglich (§6.1.5 liest ihn ausdrücklich ein). Beim
Export wird ein Wert, der Leerzeichen, `#` oder `"` enthält, nach
OpenSSH-Regeln in Anführungszeichen gesetzt und ein enthaltenes `"`
maskiert. Ohne diese Regel erzeugt ein solcher Server eine Datei, die
§3.2.6 nicht besteht.

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

### 4.6 Warum kein eigenes Feld neben der Anmeldeart

Ein früherer Entwurf dieser Spec hatte ein Feld `identity_file` am
`Server`, neben `auth`. Das ist verworfen: Mit `AuthMethod::IdentityFile
{ path, .. }` aus Spec 0076 steht der Pfad **in** der Anmeldeart, wo er
hingehört. Ein zweites Feld daneben würde denselben Wert doppelt führen,
und zwei Quellen für dieselbe Tatsache laufen früher oder später
auseinander — spätestens, wenn der Überführungsknopf aus 0076 die
Anmeldeart auf `PrivateKey` umstellt und niemand daran denkt, das Feld
mitzuziehen.

Gemessene Folge: **keine Wanderung** (§1.2). `auth_method` ist eine
JSON-Textspalte; eine Variante mehr ändert am Schema nichts.

Der Pfad wird als Zeichenkette gespeichert, **unverändert**: kein
`realpath`, keine Existenzprüfung, kein Auflösen von `~`. Das ist beim
Import Absicht (§5.5) — `ssh` löst `~` selbst auf, und jede Auflösung
unsererseits wäre eine Aussage darüber, welche Dateien auf dem Rechner
liegen. Was beim **Verbinden** mit dem Pfad geschieht, regelt Spec 0076,
nicht diese.

**Was der Nutzer nach dem Import in der Hand hat** (Weg (a) aus §3.1.9):
einen Server, der sich mit der Schlüsseldatei anmeldet, genau wie `ssh`
es täte — und einen Knopf, der daraus einen im Schlüsselbund
gespeicherten Schlüssel macht, wenn er das lieber hat. Beides kommt aus
0076; diese Spec sorgt nur dafür, dass der Import dort richtig ankommt.

## 5. Sicherheits-Invarianten

Berührt sind **Credential-Handling**, mittelbar über die erzeugten
Profile **Ausführungspfad** und **Filter-/Risiko-Bewertung**, und neu:
**Dateizugriff auf Pfade, die aus einer fremden Datei stammen**. Daher
`Review-Priorität: ERHÖHT`.

**5.1 Eine Schlüsseldatei wird nur geöffnet, wenn der Nutzer es
ausdrücklich verlangt hat — und nur die angekündigten.**

- **Von sich aus** öffnet der Import ausschließlich `ssh_config`-Dateien:
  die gewählte und die über `Include` erreichten.
- Eine Datei, auf die `IdentityFile` zeigt, wird **nur** auf Weg (b)
  aus §3.1.9 geöffnet, **nur beim Bestätigen** (nie in der Vorschau) und
  **nur**, wenn die Vorschau sie vorher mit vollem Pfad genannt hat. Eine
  Datei, die dort nicht stand, wird nicht geöffnet — auch dann nicht,
  wenn sich die Konfigurationsdatei zwischenzeitlich geändert hat.
- Auf Weg (b) geht der gelesene Inhalt **ausschließlich** in den
  Schlüsselbund: nicht in den Importplan, nicht in die Datenbank, nicht
  in ein Log, nicht in eine Fehlermeldung.
- Der Export schreibt keinen Wert, der aus dem Schlüsselbund stammt.

Nachgewiesen durch §6.4.1, §6.4.1a, §6.4.1b und §6.4.2.

**5.2 Eine fremde Datei ändert keine Sicherheitseinstellung.**
`post_ingest_policy` und `ai_injection_check_enabled` kommen
ausschließlich aus den Produktvorgaben (3.1.12). Eine `ssh_config` darf
nicht dazu führen, dass ein Profil mit schwächerer Prüfung entsteht.
Nachgewiesen durch §6.4.3.

**5.2a Ein importiertes Schlagwort kann eine bestehende Filterregel
treffen.** Das ist der **einzige** Weg, auf dem eine fremde Datei die
Filterauswertung erreicht, und er gehört benannt: `Server.tags` ist
bewusst derselbe Typ wie `Scope::Tag` der Filter-Engine
(`crates/core/src/profiles/types.rs:51-57`, so im Code kommentiert;
`crates/core/src/filter/types.rs:115-119`). Ein Muster aus der fremden
Datei wird also wörtlich zu einem Geltungsbereich, gegen den bestehende
Regeln greifen.

Entschärfend, und nachgesehen: Die Engine sortiert
`Deny`/`Confirm`/`Allow` als Stufen **vor** der Scope-Genauigkeit
(`crates/core/src/filter/engine.rs:473`) — ein `Deny` lässt sich durch
ein importiertes Schlagwort **nicht** abschwächen. Was bleibt, ist eine
Tag-`Allow`-Regel, die ein importiertes Profil von der Vorgabe
`Confirm` auf `Allow` hebt. Weil importierte Schlagworte immer `*`, `?`
oder `!` enthalten (3.1.3), ist die Trefferfläche schmal — aber nicht
leer, und „schmal" ist bei ERHÖHTER Priorität kein Argument.

**Anforderung daraus:** Die Vorschau MUSS Schlagworte, die auf eine
**bestehende** Filterregel passen, als solche kennzeichnen und die
betroffene Regel nennen. Der Nutzer kann sie einzeln abwählen.
Nachgewiesen durch §6.4.3a.

**5.3 Kein unbemerkter Jump-Host.** Jeder `jump_host`, der beim Import
entsteht, steht in der Vorschau. Eine Jump-Host-**Schleife** wird beim
Import abgelehnt, nicht erst beim Verbinden — heute prüft das nur
`jump_host.rs:27-29`, also nachdem ein Nutzer sich schon verbinden
wollte. Der Import bringt die Prüfung nach vorn (§1.3). Nachgewiesen
durch §6.4.4.

**5.4 Aus einer gelesenen Datei gelangt kein Inhalt nach draußen — auch
nicht auf den Bildschirm.**

Mit der Entscheidung E-4 bestimmt eine fremde Datei, welche weiteren
Dateien wir öffnen, ohne Einschränkung der Pfade. Das ist weniger
dramatisch, als es zunächst klingt, und die Einordnung gehört in die
Spec, damit sie nicht bei jeder Lesung neu erfunden wird:

- **Keine Rechteausweitung.** Wir lesen mit den Rechten des Nutzers und
  können nichts öffnen, was er nicht ohnehin selbst lesen kann.
- **Keine Ausführung.** `ssh_config` ist Beschreibung, kein Programm. Es
  gibt keinen Weg von einer Direktive zu einem Kommando.
- **Was bleibt, ist die Anzeige.** Eine untergeschobene Konfiguration mit
  `Include ~/.ssh/id_rsa` könnte fremden Inhalt in die Vorschau spülen,
  wo er auf einem geteilten Bildschirm, in einem Screenshot oder in einem
  Support-Anhang landet.

**Diesen einen Kanal macht der Parser zu, nicht eine Liste von Verboten**
(E-7):

1. Eine eingebundene Datei ohne erkannte Direktive, mit NUL-Byte oder
   ohne gültiges UTF-8 wird übersprungen (3.1.4a) — nicht Zeile für
   Zeile gemeldet.
2. Von einer nicht übernommenen Zeile wird nur der Direktivenname
   gemeldet, nie ihr Wert (3.1.5).
3. Übernommen werden ausschließlich die Werte aus §3.1.2 — und die
   stehen ohnehin als Serverfelder in der Vorschau, dafür ist sie da.

Ergänzend, weil es nichts kostet: Dateiliste und Direktivenliste gehen in
**keinen** KI-Prompt, in keine Telemetrie und in kein Log außerhalb des
Geräts. Und die Vorschau nennt jede gelesene Datei mit vollem Pfad
(3.1.4) — wer eine untergeschobene Datei importiert, sieht vor dem
Anlegen, was sie aufgemacht hat.

Nachgewiesen durch §6.4.9.

**5.5 Auf Weg (a) wird kein Pfad zu einer Schlüsseldatei aufgelöst.** Ein
`IdentityFile`-Wert ist dann für uns eine Zeichenkette (§4.6): kein
`realpath`, keine Existenzprüfung, kein Lesen. Damit gibt es weder
Pfaddurchquerung noch ein Orakel darüber, welche Schlüssel auf dem
Rechner liegen. Auf Weg (b) wird die Datei geöffnet — dann gelten die
Prüfungen aus Spec 0076 (Dateirechte, symbolische Links, Gültigkeit des
Schlüssels), und zwar dieselben wie beim Anlegen eines Servers von Hand.
Der Import erfindet dafür keine eigenen, schwächeren Regeln.

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
11. Zwei `IdentityFile`-Zeilen in einem Block → der erste Wert steht in
    der Anmeldeart, der zweite in der Liste der nicht übernommenen
    Direktiven (3.1.9).
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
   `port`, `user` und — für einen Server mit `AuthMethod::IdentityFile`
   — `identityfile` wie erwartet (3.2.6). **Auf den konkreten Pfad
   prüfen, nicht auf das Vorkommen des Schlüsselworts:** `ssh -G` gibt
   `identityfile`-Zeilen auch aus seinen Vorgaben aus, ein Test auf das
   bloße Schlüsselwort würde also auch grün, wenn der Export gar nichts
   geschrieben hat. `-G` statt einer echten
   Verbindung: prüft die Datei, braucht kein Netz.
3. **Rundlauf** (Import auf Weg (a)): Export → Import in einen leeren
   Bestand → Name, Adresse, Port, Benutzer, Jump-Host-Kette **und der
   Schlüsselpfad in der Anmeldeart** stimmen mit dem Original überein
   (BL-0218, Akzeptanz). Gruppe, Schlagworte und Notizen werden
   ausdrücklich **nicht** verglichen (§4.4).
3a. **Rundlauf nach Weg (b):** Ein Server, dessen Schlüssel im
   Schlüsselbund liegt, erzeugt beim Export **kein** `IdentityFile`,
   sondern den Kommentar aus 3.2.3 — und die Datei enthält kein
   Schlüsselmaterial (§6.4.2).
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
    unterschiedliche Aliase (4.3) — **und** ein Server, dessen Jump-Host
    dabei umbenannt wurde, trägt in `ProxyJump` den umbenannten Alias;
    `ssh -F … -G` besteht (3.2.1).
13. Ein `username` mit Leerzeichen wird in Anführungszeichen exportiert;
    `ssh -F … -G` liefert 0 und meldet den Benutzernamen vollständig
    (4.3).
14. `~/.ssh/config` als Exportziel wird abgelehnt, auch bei bestätigtem
    Dialog; die vorhandene Datei ist danach byte-gleich (3.2.5).
15. Ein Eintrag mit **relativem** `IdentityFile`-Pfad und einer mit
    `~user/…` erscheinen in der Vorschau als „beim Verbinden nicht
    benutzbar" und werden trotzdem angelegt (3.1.9 a).
16. Ein **verschlüsselter** Schlüssel auf Weg (b): Der Eintrag ist in
    der Vorschau als „Passphrase nachzutragen" gekennzeichnet; nach dem
    Import steht der Schlüssel verschlüsselt im Schlüsselbund und es
    gibt keine `passphrase_ref` (3.1.9 b).

### 6.4 Adversariale Fälle (Pflicht bei ERHÖHT)

1. **Weg (a) liest keine Schlüsseldatei.** Eine Konfiguration zeigt mit
   `IdentityFile` auf eine im Test angelegte Datei mit erkennbarem
   Inhalt. Nach einem Import in der Vorgabeeinstellung: Der **Pfad**
   steht in der Anmeldeart, der **Inhalt** kommt nirgends vor — nicht im
   Plan, nicht im Profil, nicht in der Datenbank, nicht im Log. Zusatz:
   Die Datei wird im Test so präpariert, dass jedes Öffnen auffällt
   (nicht lesbare Rechte), und der Import läuft trotzdem durch. Der Test
   scheitert, sobald jemand „hilfsbereit" den Schlüssel einliest.

1a. **Weg (b) liest genau die angekündigten Dateien und sonst keine.**
   Drei Server mit `IdentityFile`, davon einer abgewählt. Nach dem
   bestätigten Import: Die Schlüssel der zwei gewählten liegen im
   Schlüsselbund, der dritte nicht; **der Inhalt keiner** der drei taucht
   im Plan, in der Datenbank oder im Log auf (5.1). Weitere Fälle: eine
   fehlende Datei, eine nicht lesbare Datei und eine Datei, die kein
   gültiger Schlüssel ist → der jeweilige Server fällt auf Weg (a)
   zurück, die anderen werden normal angelegt, und die Meldung nennt den
   Grund (3.1.9).

1b. **Die Vorschau öffnet nichts.** Derselbe Aufbau wie 1a, aber der
   Nutzer bricht in der Vorschau ab. Keine der Schlüsseldateien wurde
   geöffnet (nachgewiesen über Rechte oder Zugriffszeit), und im
   Schlüsselbund liegt nichts (5.1).
2. **Kein Geheimnis im Export.** Ein Server mit Passwort und ein Server
   mit hinterlegtem Schlüssel werden exportiert; die erzeugte Datei wird
   gegen die Leak-Muster und gegen die im Test gesetzten Werte geprüft —
   kein Treffer (3.2.2).
3. **Eine fremde Datei schwächt keine Einstellung.** Eine Konfiguration
   mit `PostIngestPolicy allow`, `AiInjectionCheck no` und denselben
   Werten als `# smart-ssh:`-Kommentar. Ergebnis: beide Felder stehen
   auf den Produktvorgaben (5.2). Der Test scheitert, sobald der Import
   irgendeinen Weg öffnet, sie zu setzen.
3a. **Ein importiertes Schlagwort trifft eine bestehende Filterregel.**
   Im Bestand liegt eine Tag-Regel mit `Allow` auf `*.prod.de`; die
   importierte Datei enthält einen Platzhalterblock `Host *.prod.de`.
   Erwartung: Die Vorschau kennzeichnet das Schlagwort und nennt die
   Regel (5.2a); der Nutzer kann es abwählen; wählt er ab, trägt das
   Profil das Schlagwort nicht. Zweiter Fall: eine `Deny`-Regel — sie
   bleibt in jedem Fall wirksam (Stufen vor Scope-Genauigkeit,
   `engine.rs:473`).

4. **Jump-Host-Schleife.** `a` springt über `b`, `b` über `a`. Der
   Import legt die Schleife **nicht** an und sagt warum (5.3). Zweiter
   Fall: eine Kette über drei Hosts zurück auf den ersten. Dritter Fall:
   die Schleife verteilt über zwei `Include`-Dateien. In allen dreien
   wird `ProxyJump` bei **allen** beteiligten Einträgen als nicht
   übernommen gemeldet, und die Profile selbst entstehen normal (3.1.6).

4a. **Mehr-Hop-Ketten: richtige Richtung, keine fremden Profile.** (a)
   `Host x` mit `ProxyJump a,b,c`, alle neu im Import → es entstehen
   **genau** die Kanten `x→c`, `c→b`, `b→a`, und `a` hat kein
   `jump_host` (3.1.6). Der Test prüft jede Kante einzeln; ohne das
   würde eine seitenverkehrt gebaute Kette grün.
   (b) `b` liegt bereits im Bestand → Kette wird **nicht** angelegt,
   gemeldet, und `b` ist danach Feld für Feld unverändert. (c) `b` kommt
   in zwei Ketten mit verschiedenen Vorgängern vor → beide gemeldet,
   keine angelegt. (d) `b` ist Zwischenstation **und** trägt im eigenen
   Block ein `ProxyJump` → zweite Quelle, dieselbe Rechtsfolge (3.1.6).
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
6. **Bösartige Aliase, Importseite.** `Host ../../etc/passwd`,
   `Host $(whoami)`, `Host a b` mit Steuerzeichen, ein Alias mit `\n` im
   Wert. Ergebnis: entweder abgelehnt oder als harmloser Name
   übernommen — in keinem Fall entsteht daraus ein Pfad oder ein
   Kommando.

6a. **Dieselben Namen, Exportseite.** Ein Server, dessen Name `\n`, `#`
   oder ein Anführungszeichen enthält, erzeugt beim Export **keine**
   zweite Zeile und keinen Kommentarabbruch; `ssh -F … -G` besteht
   (4.3). Getrennt von 6, weil der Schreiber erst in Schritt 4 entsteht
   und Schritt 1 sonst nicht für sich prüfbar wäre.
7. **Unsichtbare Randzeichen.** (a) Ein `HostName` mit BOM und
   Zero-Width-Space an den **Rändern** → die Zeichen sind entfernt, der
   Wert ist der erwartete (3.1.2, Absatz „Randzeichen"). (b) Dieselben
   Zeichen **innerhalb** des Werts → bleiben erhalten, der Wert weicht
   vom bereinigten ab. (c) Ein Wert, der **nur** aus solchen Zeichen
   besteht → gilt als leer, also als „nicht angegeben". Ohne (b) und (c)
   hätte der Test kein Soll und würde bei jeder Implementierung grün —
   genau der Fehler, den §6 sonst ausschließt.
8. **Der lokale Pseudo-Server als Jump-Host.** Eine Konfiguration, deren
   `ProxyJump` auf den Namen des lokalen Servers zeigt → abgelehnt mit
   `SERVER_JUMP_HOST_LOCAL` (5.7).
9. **Fremdinhalt erscheint nirgends, auch nicht in der Vorschau.** Vier
   Fälle, je mit `Include` auf eine Datei, die kein `ssh_config` ist:
   (a) ein privater Schlüssel im OpenSSH-Format; (b) eine Textdatei mit
   einer erkennbaren Zeichenfolge; (c) eine Binärdatei mit NUL-Bytes;
   (d) eine Datei mit ungültigem UTF-8. In **allen** Fällen gilt: Die
   erkennbare Zeichenfolge bzw. jeder Byte-Inhalt der Datei taucht
   **nirgends** auf — nicht in der Vorschau, nicht im Plan, nicht in
   einer Meldung, nicht im Log (5.4, 3.1.4a). Was auftaucht, ist genau
   zweierlei: der volle **Pfad** der Datei und der Satz
   „keine ssh_config, übersprungen".

9a. **Der Direktivenname verrät den Wert nicht.** Eine gültige
   `ssh_config` mit `Compression yes` und einer Zeile
   `UnknownDirective <geheimnisverdächtiger Wert>`. Die Liste der nicht
   übernommenen Direktiven enthält `Compression` und
   `UnknownDirective` mit Zeilennummern — und **keinen** der beiden
   Werte (3.1.5).

## 7. Umsetzungsreihenfolge

**Vorbedingung: Spec 0076 ist umgesetzt** — mindestens ihr Teil, der
`AuthMethod::IdentityFile` einführt und persistiert. Ohne sie fehlt Weg
(a) aus §3.1.9; der Import wäre dann auf (b) und (c) beschränkt und der
Rundlauf (§6.3.3) nicht fahrbar.

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
   (3.1.6) inklusive der Kantenrichtung, Konflikten (3.1.8),
   Zyklusprüfung (5.3). Tests §6.1, §6.4.3, §6.4.4, §6.4.4a, §6.4.6,
   §6.4.7. *(opus — hier entsteht die Logik, gegen die §6.4 fährt)*
2. **Dateizugriff in `app-shell`**: `Include`-Auflösung mit Tiefe,
   Schleifenerkennung und Platzhalterpfaden (3.1.4), Übergehen von
   Nicht-`ssh_config`-Dateien (3.1.4a), Gesamtgrenzen (§3.3),
   Gruppenbaum (§4.5). Tests §6.2, §6.4.5, §6.4.9, §6.4.9a. *(opus —
   hier liegt die neue Angriffsfläche)*
3. **Kommandos**: `preview_ssh_config_import`,
   `apply_ssh_config_import`, `export_ssh_config`; die drei Wege aus
   §3.1.9 inklusive Rückfall auf (a); Rollback für Profile **und**
   Gruppen (3.1.11); `is_local` ausschließen. Tests §6.3.4–9,
   §6.3.15–16, §6.4.1, §6.4.1a, §6.4.1b, §6.4.3a, §6.4.8. *(opus —
   Credential-Nähe, und der einzige Schritt, in dem überhaupt eine
   Schlüsseldatei geöffnet wird)*
4. **Schreiber** (Export) inklusive Alias- und Wert-Regel (4.3),
   `ProxyJump`-Alias (3.2.1), Ablehnung von `~/.ssh/config` (3.2.5) und
   Kommentaren (3.2.3). Tests §6.3.1–3, §6.3.3a, §6.3.10–14, §6.4.2,
   §6.4.6a. *(sonnet)*

   **Warum die Export-Tests hier stehen und nicht früher:** §6.3.10
   („kommt in der exportierten Datei nicht vor"), §6.4.2 (erzeugte Datei
   gegen die Leak-Muster) und §6.4.6a brauchen alle den Schreiber. In
   einem früheren Schritt zugeordnet, wäre „jeder Schritt lässt das Gate
   grün" nicht haltbar.
5. **Frontend**: Vorschau-Dialog mit Abwahl je Eintrag, Konfliktanzeige,
   Dateiliste, Gruppenbaum, Liste der nicht übernommenen Direktiven; die
   Wahl zwischen den drei Wegen aus §3.1.9 samt der Liste der Dateien,
   die auf Weg (b) geöffnet würden; Kennzeichnung relativer Pfade und
   nachzutragender Passphrasen (3.1.9); Kennzeichnung von Schlagworten,
   die bestehende Filterregeln treffen (5.2a); Export-Dialog mit der
   `Include`-Zeile. i18n vollständig. *(sonnet)*
6. **`CHANGELOG.md`** unter `[Unreleased]`, Nutzersicht. *(sonnet)*

Der Rundlauftest (§6.3.3) läuft ab Schritt 4 und ist die Abnahme für
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

**2026-09-23 · E-2 · `IdentityFile`** *(ersetzt eine frühere Fassung
desselben Punktes)*. Stefan: Eine Schlüsseldatei wird **als solche
übernommen** und von der Platte gelesen — das ist eine **weitere
Anmeldeart**, nicht ein Merkposten. Sie ist in der Oberfläche wählbar
(„Privater Schlüssel" / „Schlüsseldatei"), sonst ließe sie sich nur durch
einen Import erzeugen. Dazu kommt ein Knopf, der eine Schlüsseldatei mit
einem Druck in einen gespeicherten Schlüssel überführt. Beim Import wird
gefragt, was der Nutzer will.

Daraus wurden **zwei Specs** (Stefan: „mach daraus mehrere specs wenn du
willst"): **0076** führt die Anmeldeart, ihre Oberfläche und den
Überführungsknopf ein; **0075** benutzt sie und stellt die Frage beim
Import. 0076 wird zuerst umgesetzt (§7, Vorbedingung).

Die frühere Fassung — Pfad in einem eigenen Feld merken, nicht benutzen —
ist damit hinfällig. Sie hätte denselben Wert doppelt geführt (§4.6).
Eingearbeitet in Kopf, §1.2, §2 (Nicht-Ziele 4 und 5), §3.1.2, §3.1.9,
§3.2.1, §3.2.3, §4.6, §5.1, §5.5, §6.3.2–3a, §6.4.1/1a/1b und §7.

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

**2026-09-23 · E-5 · Zuschnitt von Spec 0076.** Aus E-2: 0076 umfasst
(1) die Anmeldeart `AuthMethod::IdentityFile { path, passphrase_ref }`
samt allem, was beim Verbinden daran hängt — Dateirechte, symbolische
Links, Passphrase, fehlende Datei; (2) ihre Wahl in der Oberfläche beim
**Anlegen und Bearbeiten** eines Servers, nicht nur über den Import;
(3) den Knopf, der eine Schlüsseldatei einliest, den Inhalt in den
Schlüsselbund legt und die Anmeldeart auf `PrivateKey` umstellt. Die
Datei bleibt dabei unberührt; was sich ändert, ist nur, woher Smart SSH
den Schlüssel nimmt.

**2026-09-23 · E-6 · Wahl beim Import.** Stefan: „beim import können wir
fragen, wie die es wollen." Umgesetzt als die drei Wege in §3.1.9, mit
(a) als Vorgabe — es ist der einzige, der keine zusätzliche Datei
öffnet, und der Nutzer kann jeden Server später einzeln mit dem Knopf
aus 0076 umstellen. Die Wahl gilt für den ganzen Import und ist je
Eintrag umstellbar.

**2026-09-23 · E-7 · Der Parser filtert, statt dass Regeln es auffangen.**
Stefan, zu E-4: „warum ist das mit den includes so gefährlich? sollte das
der parser nicht filtern? also wenn die kein ssh format haben, ignoriert
der die?" — Berechtigt, und die frühere Fassung von §5.4 hat die Gefahr
überzeichnet und die Abhilfe an der falschen Stelle gesucht. Es gibt
weder Rechteausweitung noch Ausführung; der einzige reale Kanal war die
**Anzeige** — und zwar durch meine eigene Anforderung 3.1.5, die
unbekannte Zeilen mitsamt Inhalt in die Vorschau geschrieben hätte.

Umgesetzt: Eine eingebundene Datei, die keine `ssh_config` ist, wird
übersprungen statt Zeile für Zeile gemeldet (3.1.4a), und von einer nicht
übernommenen Zeile erscheint nur der Direktivenname, nie sein Wert
(3.1.5). Damit kann kein Fremdinhalt mehr auf den Schirm, und §5.4 ist
von einer Liste von Verboten auf eine Eigenschaft des Parsers
geschrumpft. Zusätzliche Tests: §6.4.9 (vier Fälle) und §6.4.9a.

*Anmerkung des Architekten zur Grenze:* „Unbegrenzt viele Dateien"
bezieht sich auf die **Anzahl**. Die Gesamtgrenzen aus §3.3 (Bytes,
Zeilen, `Host`-Blöcke) gelten weiterhin — aber ausdrücklich **über alle
Dateien zusammen** statt je Datei, weil eine Grenze je Datei durch eine
Kette von `Include`-Dateien umgangen würde. Ohne irgendeine
Gesamtgrenze wäre §5.6 nicht haltbar. Stefan am 2026-09-23 vorgelegt,
kein Widerspruch — die Grenzen aus §3.3 gelten damit als entschieden,
§8 bleibt bei „keine offenen Punkte".
