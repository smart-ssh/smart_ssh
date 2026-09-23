# Spec 0076 — Anmeldung mit einer Schlüsseldatei

Status: Vorschlag (Architekt, K3 entschieden) · Backlog: BL-0221, BL-0222 · Gate: release-1.0/D
Repo: **öffentlich** `smart-ssh` — `crates/core/src/ssh/auth.rs` und
`crates/core/src/profiles/types.rs` (Anmeldeart und Auflösung),
`crates/ssh-transport/` (Aufrufstelle), `crates/app-shell/` (Dateizugriff,
Kommandos, DTOs), Frontend
Review-Priorität: **ERHÖHT** (neue Anmeldeart im Ausführungspfad,
Credential-Handling, und die erste Stelle, an der Smart SSH eine
Schlüsseldatei von der Platte liest; adversariale Fälle in §6.4)
**Voraussetzung für Spec 0075** — deren Import benutzt diese Anmeldeart.

> Smart SSH kennt Schlüssel heute nur als **Inhalt** im Schlüsselbund. Wer
> aus der `ssh`-Welt kommt, verwaltet sie als **Dateien** und will einen
> Pfad angeben. Diese Spec macht daraus eine fünfte Anmeldeart — und gibt
> dem Nutzer einen Knopf, mit dem er jederzeit in die andere Richtung
> wechselt.

## 1. Ausgangslage (belegt)

### 1.1 Vier Anmeldearten, keine dateibasiert

`AuthMethod` (`crates/core/src/profiles/types.rs:114-127`):
`Password { credential_ref }`, `PrivateKey { credential_ref,
passphrase_ref }`, `Agent`, `Certificate { cert_ref, key_ref }`. Jedes
`CredentialRef` (`types.rs:137`) ist ein Schlüsselbund-Schlüssel, kein
Pfad. Die Eingabe heißt `key_content` (`crates/app-shell/src/dto.rs`,
`AuthMethodInput::PrivateKey`), und beim Verbinden wird dieser Inhalt
geparst:

```rust
// crates/ssh-transport/src/auth.rs:69
let parsed = PrivateKey::from_openssh(key.expose_secret().as_bytes())
```

**Nirgends liest Smart SSH heute eine Schlüsseldatei von der Platte.**

### 1.2 Die eine Stelle, die `AuthMethod` auflöst

`resolve_auth` (`crates/core/src/ssh/auth.rs:39`):

```rust
pub fn resolve_auth(
    auth: &AuthMethod,
    credentials: &dyn CredentialStore,
) -> Result<ResolvedAuth, SshError>
```

Sie bildet auf `ResolvedAuth` (`auth.rs:21-32`) ab:
`Password(SecretString)`, `PrivateKey { key: SecretString, passphrase:
Option<SecretString> }`, `Agent`, `Certificate { cert, key }`.

**Produktions-Aufrufstellen: genau eine** —
`crates/ssh-transport/src/auth.rs:21`, `resolve_auth(&hop.auth,
credentials)?`, je Hop der Verbindungskette. Dazu vier Aufrufe in
`crates/core/src/ssh/tests.rs:366,381,391,405`.

Daraus folgt der Zuschnitt dieser Spec fast von selbst (§4.1): Der
Eingriff ist auf eine Funktion, eine Aufrufstelle und vier Tests
begrenzt.

### 1.3 `resolve_auth` liegt in `core` — und `core` darf nicht ans Dateisystem

Die Architekturregel der `CLAUDE.md` ist eindeutig: „`core` never depends
on Tauri or a UI framework", und „Every external boundary is a trait,
defined before its concrete implementation." Eine Schlüsseldatei zu lesen
ist eine äußere Grenze wie `SshTransport`, `CredentialStore` oder
`ProfileStore`. Sie bekommt deshalb eine Schnittstelle, keinen
`std::fs::read` mitten in `core` (§4.2).

### 1.4 Keine Wanderung nötig

`auth_method` ist eine **JSON-Textspalte**
(`crates/persistence-sqlite/migrations/0001_initial.sql:31`: „JSON-
serialisiertes AuthMethod-Enum"), serialisiert über
`auth_method_to_json`/`auth_method_from_json`
(`crates/persistence-sqlite/src/mapping.rs:9-17`). Eine Variante mehr
ändert am Schema nichts.

**Eine Folge davon gehört benannt:** Eine ältere Programmfassung, die
denselben Datenbestand öffnet, kann einen Server mit der neuen Variante
**nicht** lesen — `auth_method_from_json` scheitert. Das trifft jeden, der
nach einem Update zurückgeht. Anforderung A-9 regelt, dass daraus eine
verständliche Meldung wird und kein Absturz.

## 2. Ziel und Nicht-Ziele

**Ziel.** Ein Server kann sich mit einer Schlüsseldatei anmelden, so wie
`ssh` es täte. Die Anmeldeart ist in der Oberfläche wählbar, und ein Knopf
überführt sie jederzeit in einen im Schlüsselbund gespeicherten Schlüssel.

**Nicht-Ziele:**

1. **Kein Rückweg.** Aus einem gespeicherten Schlüssel wieder eine
   Schlüsseldatei zu machen ist nicht vorgesehen — das hieße,
   Schlüsselmaterial aus dem Schlüsselbund auf die Platte zu schreiben.
2. **Kein Verwalten von Schlüsseldateien.** Wir erzeugen keine, löschen
   keine, ändern keine Rechte. Wir lesen.
3. **Kein Agent-Ersatz.** Wer `ssh-agent` benutzt, bleibt bei
   `AuthMethod::Agent`; diese Spec ändert daran nichts.
4. **Kein Import.** Das ist Spec 0075, die diese hier voraussetzt.
5. **Keine Änderung am Transport.** `ssh-transport` bekommt dieselbe
   `ResolvedAuth::PrivateKey` wie heute (§4.1).

## 3. Anforderungen

### 3.1 Teil A — die Anmeldeart

**A-1** `AuthMethod` bekommt eine fünfte Variante:

```rust
IdentityFile {
    path: String,
    passphrase_ref: Option<CredentialRef>,
}
```

`path` wird gespeichert, **wie der Nutzer ihn angegeben hat**: kein
`realpath`, keine Normalisierung, kein Auflösen von `~` beim Speichern.
Aufgelöst wird erst beim Lesen (A-3).

**A-2** `resolve_auth` bildet `IdentityFile` auf
**`ResolvedAuth::PrivateKey { key, passphrase }`** ab — dieselbe Variante
wie ein gespeicherter Schlüssel. `ssh-transport` und `russh` sehen keinen
Unterschied; es gibt **keine** neue `ResolvedAuth`-Variante (§4.1).

**A-3** Beim Auflösen wird die Datei gelesen. Dabei gilt:

- `~` und `~user` am Anfang werden aufgelöst; ein relativer Pfad wird
  **abgelehnt** (Fehlermeldung), weil „relativ wozu" beim Verbinden keine
  beantwortbare Frage ist.
- Symbolischen Links wird gefolgt — `ssh` tut das auch (OP-2).
- Die Datei wird **bei jedem Verbindungsaufbau** gelesen, nicht
  zwischengespeichert (OP-3). Ein getauschter Schlüssel wirkt damit
  sofort, und nichts liegt länger im Speicher als nötig.
- Größe höchstens **1 MiB**. Darüber: Fehlermeldung, kein Lesen in den
  Speicher.

**A-4 Dateirechte.** Ist die Datei auf einem Unix-System für Gruppe oder
Welt lesbar, wird die Anmeldung **abgelehnt** — mit derselben Begründung
wie `ssh` („unprotected private key file"), in eigener Formulierung und
mit dem `chmod`-Befehl, der es behebt. Auf Windows entfällt die Prüfung,
wie bei OpenSSH. Entscheidbar in OP-1.

**A-5** Die Passphrase wird wie bei `PrivateKey` behandelt: optional im
Schlüsselbund unter `passphrase_ref`. Ist keine hinterlegt und der
Schlüssel ist verschlüsselt, ergibt das eine Meldung, die genau das sagt
— nicht „Anmeldung fehlgeschlagen".

**A-6 Fehlerpfade, jeder mit eigener Meldung**, und **keine** davon
enthält Dateiinhalt (§5.2):

| Fall | Meldung nennt |
|---|---|
| Datei fehlt | den Pfad, „nicht gefunden" |
| keine Leseberechtigung | den Pfad, „nicht lesbar" |
| Rechte zu weit (A-4) | den Pfad und den `chmod`-Befehl |
| größer als 1 MiB | den Pfad und die Grenze |
| kein gültiger OpenSSH-Schlüssel | den Pfad, „kein gültiger Schlüssel" |
| verschlüsselt, keine Passphrase | den Pfad, „Passphrase nötig" |
| Passphrase falsch | „Passphrase falsch" |
| relativer Pfad | den Pfad, „absoluter Pfad nötig" |

**A-7** Fehler beim Lesen sind `SshError::CredentialResolutionFailed`,
wie jeder andere Auflösungsfehler heute (`auth.rs:44-71`). Kein neuer
Fehlertyp, kein Panic.

**A-8 Die Anmeldeart gilt auch für Jump-Hosts.** `resolve_auth` läuft je
Hop (`ssh-transport/src/auth.rs:21`); ein Jump-Host mit Schlüsseldatei
funktioniert damit ohne Zusatzarbeit. Die Fehlermeldung MUSS sagen,
**welcher** Hop gescheitert ist.

**A-9 Ältere Programmfassungen.** Trifft `auth_method_from_json` auf eine
unbekannte Variante, ergibt das eine verständliche Meldung („dieser Server
wurde mit einer neueren Fassung angelegt"), keinen Absturz und keinen
Datenverlust: Der Server bleibt in der Datenbank, wird in der Liste als
nicht benutzbar markiert und lässt sich löschen.

### 3.2 Teil B — in der Oberfläche

**B-1** Beim **Anlegen und Bearbeiten** eines Servers ist
„Schlüsseldatei" neben „Passwort", „Privater Schlüssel", „Agent" und
„Zertifikat" wählbar. Ohne das gäbe es einen Zustand, den nur ein Import
herstellen kann — und genau das soll es nicht geben (BL-0221).

**B-2** Die Eingabe besteht aus einem Pfadfeld **und** einem
Dateidialog-Knopf. Getippte Pfade bleiben erlaubt: Wer `~/.ssh/id_ed25519`
tippt, will nicht klicken.

**B-3** Nach der Auswahl zeigt die Oberfläche **vor dem Speichern** an,
was an der Datei auffällt: ob sie existiert, ob die Rechte passen (A-4),
ob sie wie ein OpenSSH-Schlüssel aussieht und ob sie verschlüsselt ist.
Dafür wird sie gelesen — der Inhalt wird jedoch **nur** für diese
Feststellungen benutzt, nirgends gespeichert und nirgends angezeigt
(§5.2). Ein Fehlbefund hindert das Speichern **nicht**: Die Datei darf
erst später entstehen.

**B-4** In der Serverliste und in den Serverdetails ist erkennbar, dass
die Anmeldung an einer Datei hängt, und **wo** die Datei liegt.

**B-5** i18n vollständig für `de` und `en`; keine fest verdrahteten Texte.

### 3.3 Teil C — Überführung in einen gespeicherten Schlüssel (BL-0222)

**C-1** Bei einem Server mit `AuthMethod::IdentityFile` gibt es einen
Knopf „In den Schlüsselbund übernehmen".

**C-2** Vorher zeigt ein Dialog: **welche Datei** gelesen wird (voller
Pfad), **was sich ändert** (die Anmeldung hängt danach am Schlüsselbund,
nicht mehr an der Datei) und **was nicht** (die Datei bleibt unverändert
liegen).

**C-3** Beim Bestätigen: Datei einmal lesen, Inhalt **unverändert** in den
Schlüsselbund legen, Anmeldeart auf `PrivateKey { credential_ref,
passphrase_ref }` umstellen. Eine vorhandene `passphrase_ref` wandert
unverändert mit.

**C-4 Ein verschlüsselter Schlüssel wird verschlüsselt übernommen.** Der
Dateiinhalt geht byte-gleich in den Schlüsselbund; entschlüsselt wird
nichts, gespeichert wird nichts Entschlüsseltes. Das entspricht dem
heutigen Verhalten bei `key_content`.

**C-5 Die Ursprungsdatei wird nicht angefasst** — nicht gelöscht, nicht
geändert, nicht in den Rechten verändert. Ein Angebot, sie zu löschen,
ist **nicht** Teil dieser Spec (OP-5).

**C-6** Schlägt das Schreiben in den Schlüsselbund fehl, bleibt der Server
unverändert auf `IdentityFile` — kein halber Zustand. Das entspricht dem
Rollback-Verhalten in `crates/app-shell/src/servers.rs:44-54`.

**C-7** Die Überführung ist erst wählbar, wenn die Datei tatsächlich
lesbar und ein gültiger Schlüssel ist; sonst nennt der Knopf den Grund,
warum er nicht geht (A-6).

## 4. Design

### 4.1 Warum der Transport unberührt bleibt

`AuthMethod::IdentityFile` löst zu `ResolvedAuth::PrivateKey` auf (A-2).
Das ist die wichtigste Entwurfsentscheidung dieser Spec, und sie kostet
nichts: Der Unterschied zwischen „Schlüssel aus dem Schlüsselbund" und
„Schlüssel aus einer Datei" ist die **Herkunft**, nicht das Material. Was
`russh` am Ende bekommt, ist in beiden Fällen ein OpenSSH-Schlüssel als
`SecretString`, geparst von derselben Zeile
(`ssh-transport/src/auth.rs:69`).

Folge: `crates/ssh-transport/` bekommt **keine neue Verzweigung**, und die
Anmeldelogik, die heute funktioniert und getestet ist, bleibt Wort für
Wort stehen. Eine eigene `ResolvedAuth::IdentityFile`-Variante wäre eine
zweite Fassung derselben Sache — verworfen.

### 4.2 Die Schnittstelle zum Dateisystem

`core` darf nicht ans Dateisystem (§1.3). Also:

```rust
// crates/core/src/ssh/auth.rs
pub trait KeyFileReader {
    /// Liest die Schlüsseldatei unter `path`. Der Aufrufer prüft Pfad,
    /// Rechte und Größe; diese Funktion gibt nur zurück, was dort steht.
    fn read(&self, path: &str) -> Result<SecretString, KeyFileError>;
}
```

`resolve_auth` bekommt einen Parameter mehr:

```rust
pub fn resolve_auth(
    auth: &AuthMethod,
    credentials: &dyn CredentialStore,
    key_files: &dyn KeyFileReader,
) -> Result<ResolvedAuth, SshError>
```

Das ist derselbe Bau wie bei `SshTransport`, `CredentialStore` und
`ProfileStore` — die Regel der `CLAUDE.md`, nicht eine Erfindung dieser
Spec. Die konkrete Umsetzung liegt in `app-shell` (oder einer eigenen
Kiste, falls sie dort stört); in Tests steht eine Attrappe, mit der sich
jeder Fehlerfall aus A-6 ohne echte Datei herstellen lässt.

**Wer prüft was:** Die Umsetzung von `KeyFileReader` prüft Pfadform,
Rechte (A-4) und Größe (A-3) — dort, wo das Dateisystem ist. `core` prüft
nichts davon nach, sondern verarbeitet nur das Ergebnis. Zwei Stellen mit
halben Prüfungen sind schlimmer als eine mit ganzen.

**Genau eine Produktions-Aufrufstelle** ist anzupassen
(`ssh-transport/src/auth.rs:21`) plus vier Tests
(`core/src/ssh/tests.rs:366,381,391,405`) — gemessen, §1.2.

### 4.3 Warum bei jedem Verbinden gelesen wird

Alternative wäre, den Inhalt nach dem ersten Lesen im Speicher zu halten.
Verworfen aus zwei Gründen: Ein Nutzer, der seinen Schlüssel tauscht,
erwartet, dass die nächste Verbindung den neuen benutzt — so verhält sich
`ssh`. Und ein Schlüssel, der zwischen zwei Verbindungen im Speicher
liegt, ist ein Schlüssel, der in einem Speicherabbild stehen kann. Lesen
kostet Millisekunden; das ist kein Tauschgeschäft, das sich lohnt.

### 4.4 Warum der Pfad unverändert gespeichert wird

Speicherten wir den aufgelösten Pfad, hinge der Server an dem Rechner,
auf dem er angelegt wurde: `~/.ssh/id_ed25519` wird auf zwei Rechnern zu
zwei verschiedenen Pfaden. Der Nutzer hat `~` getippt und meint `~`.
Aufgelöst wird deshalb beim Lesen, nicht beim Speichern.

### 4.5 Was diese Spec **nicht** löst

Ein Server mit Schlüsseldatei ist genau so stark wie die Datei. Liegen
ihre Rechte zu weit, hilft A-4 beim Verbinden — aber eine Datei, die
zwischen zwei Verbindungen verändert wird, bemerken wir nicht, und das
ist auch nicht unsere Aufgabe. Wer das nicht will, benutzt C-1 und legt
den Schlüssel in den Schlüsselbund. Genau dafür ist der Knopf da, und
genau so gehört er in der Oberfläche begründet.

## 5. Sicherheits-Invarianten

Berührt: **Credential-Handling** und **Ausführungspfad** (die Anmeldung
steht am Anfang jeder Verbindung). Daher `Review-Priorität: ERHÖHT`.

**5.1 Schlüsselmaterial bleibt in `SecretString`.** Der gelesene Inhalt
wird nie in einen `String`, nie in eine Struktur mit abgeleitetem `Debug`
und nie über eine Grenze gereicht, die ihn protokolliert. `KeyFileReader`
gibt `SecretString` zurück, nicht `Vec<u8>` — damit gibt es keine
Zwischenform, die versehentlich irgendwo landet. Nachgewiesen durch
§6.4.1.

**5.2 Keine Fehlermeldung enthält Dateiinhalt.** Jede Meldung aus A-6
nennt **Pfad und Grund**, nie eine Zeile aus der Datei. Das betrifft
besonders „kein gültiger Schlüssel": Die Versuchung, den Anfang der Datei
mitzuliefern, ist genau der Fehler. Nachgewiesen durch §6.4.2.

**5.3 Was nicht gelesen werden darf, wird nicht benutzt.** Schlägt eine
Prüfung aus A-3 oder A-4 fehl, wird die Anmeldung **abgebrochen**, nicht
auf ein anderes Verfahren zurückgestuft. Kein stiller Rückfall auf
`Agent`, kein Weiterprobieren mit leerem Schlüssel — eine fehlgeschlagene
Anmeldung ist ein Fehler, keine Gelegenheit zur Selbsthilfe.
Nachgewiesen durch §6.4.3.

**5.4 Der Pfad wird nicht zum Orakel.** Die Meldungen aus A-6
unterscheiden „nicht gefunden" von „nicht lesbar" — das ist nötig, damit
der Nutzer weiß, was zu tun ist, und unbedenklich, weil er ohnehin mit
seinen eigenen Rechten liest. Was **nicht** passiert: Verzeichnisse
durchsuchen, benachbarte Dateien vorschlagen oder gar automatisch
`~/.ssh/id_*` probieren. Gelesen wird genau der eine angegebene Pfad.

**5.5 Die Überführung verändert die Ursprungsdatei nicht** (C-5) und
speichert nichts Entschlüsseltes (C-4). Nachgewiesen durch §6.4.4 und
§6.4.5.

**5.6 Escalation only.** Diese Spec fügt eine Anmeldeart hinzu und
schwächt keine bestehende Prüfung ab. Insbesondere bleibt jede
Filter-, Risiko- und Bestätigungslogik unverändert: Ein Server, der sich
mit einer Schlüsseldatei anmeldet, wird danach behandelt wie jeder
andere — keine Verzweigung auf die Anmeldeart irgendwo in der
Sicherheitslogik.

## 6. Tests

### 6.1 Auflösung (`core`, mit Attrappe statt Dateisystem)

1. `IdentityFile` mit gültigem Schlüssel → `ResolvedAuth::PrivateKey` mit
   genau dem Inhalt der Attrappe. **Scheitert bei kaputter
   Implementierung**, weil jede andere Variante sofort auffällt.
2. `IdentityFile` mit `passphrase_ref` → Passphrase kommt aus dem
   `CredentialStore`, nicht aus der Datei.
3. Attrappe meldet „nicht gefunden" → `CredentialResolutionFailed`, kein
   Panic (A-7).
4. Die vier bestehenden Tests (`tests.rs:366,381,391,405`) laufen nach der
   Signaturänderung unverändert im Ergebnis.
5. Jeder Fehlerfall aus A-6 ergibt genau eine unterscheidbare Meldung.

### 6.2 Dateizugriff (Umsetzung von `KeyFileReader`)

1. Absoluter Pfad, Rechte `0600`, gültiger Schlüssel → gelesen.
2. `~/…` wird aufgelöst; derselbe Schlüssel wird gefunden.
3. Relativer Pfad → Fehler „absoluter Pfad nötig" (A-3).
4. Rechte `0644` auf Unix → Fehler mit `chmod`-Befehl (A-4); auf Windows
   übersprungen.
5. Symbolischer Link auf eine gültige Datei → gefolgt (A-3).
6. Datei über 1 MiB → Fehler, und die Umsetzung liest sie **nicht**
   vollständig in den Speicher (Prüfung über die Metadaten vor dem Lesen).
7. Verzeichnis statt Datei → Fehler, kein Panik.

### 6.3 Oberfläche und Überführung

1. Ein Server mit Schlüsseldatei lässt sich anlegen, speichern, neu laden
   und bearbeiten; die Anmeldeart überlebt den Rundlauf durch die
   Datenbank (JSON, §1.4).
2. Der Vorab-Befund aus B-3 meldet fehlende Datei, zu weite Rechte und
   „verschlüsselt" richtig — und hindert das Speichern nicht.
3. Überführung eines **unverschlüsselten** Schlüssels: Anmeldeart danach
   `PrivateKey`, Verbindung gegen die Testumgebung unverändert
   erfolgreich.
4. Überführung eines **passphrase-geschützten** Schlüssels: Der Inhalt im
   Schlüsselbund ist **byte-gleich** mit der Datei, die `passphrase_ref`
   ist mitgewandert, die Verbindung klappt (C-3, C-4).
5. Schreiben in den Schlüsselbund schlägt fehl → Server unverändert auf
   `IdentityFile` (C-6).
6. Ein Jump-Host mit Schlüsseldatei verbindet; schlägt er fehl, nennt die
   Meldung **den Hop** (A-8).
7. Eine unbekannte `auth_method`-Variante in der Datenbank ergibt eine
   Meldung und einen löschbaren Eintrag, keinen Absturz (A-9).

### 6.4 Adversariale Fälle (Pflicht bei ERHÖHT)

1. **Kein Schlüsselmaterial außerhalb von `SecretString`.** Eine
   Schlüsseldatei mit erkennbarem Inhalt wird zum Verbinden benutzt.
   Danach: Der Inhalt steht **nicht** in der Datenbank, nicht im Log,
   nicht in einem `Debug`-Ausdruck des Servers, nicht in der Oberfläche
   (5.1). Der Test scheitert, sobald jemand den Schlüssel für eine
   Fehlermeldung „nur kurz" in einen `String` legt.
2. **Keine Fehlermeldung verrät Inhalt.** Fünf Dateien: leer, Textdatei,
   Binärdatei mit NUL-Bytes, ein **öffentlicher** Schlüssel statt eines
   privaten, ein abgeschnittener privater Schlüssel. Jede ergibt „kein
   gültiger Schlüssel" mit Pfad — und in **keiner** Meldung steht ein
   Byte aus der Datei (5.2).
3. **Kein Rückfall bei fehlgeschlagener Prüfung.** Rechte zu weit, Datei
   fehlt, Datei zu groß, relativer Pfad: In jedem Fall wird die
   Verbindung **abgebrochen**. Der Test prüft ausdrücklich, dass **nicht**
   auf `Agent` oder einen leeren Schlüssel zurückgefallen wird (5.3).
4. **Die Überführung lässt die Datei in Ruhe.** Prüfsumme und Rechte der
   Ursprungsdatei vor und nach C-3 sind gleich (5.5).
5. **Nichts Entschlüsseltes im Schlüsselbund.** Ein passphrase-geschützter
   Schlüssel wird überführt; der abgelegte Wert beginnt weiterhin mit
   `-----BEGIN OPENSSH PRIVATE KEY-----` und ist byte-gleich mit der
   Datei — nicht die entschlüsselte Form (C-4, 5.5).
6. **Pfad-Spielereien.** `/dev/stdin`, `/dev/zero`, ein benanntes Rohr
   (FIFO), ein Link auf ein Verzeichnis, ein Pfad mit NUL-Byte, ein Pfad
   mit `..` auf eine Datei außerhalb von `~`. Ergebnis: entweder ein
   sauberer Fehler oder ein gelesener Schlüssel — in keinem Fall ein
   Hänger, ein Panic oder ein unbegrenzter Lesevorgang.
7. **Wettlauf beim Lesen.** Die Datei wird zwischen Rechteprüfung und
   Lesen gegen eine andere ausgetauscht. Erwartung: Entweder der alte
   oder der neue Inhalt, nie ein halber, und die Rechteprüfung greift auf
   derselben Datei, die gelesen wird (offenes Dateihandle statt zweier
   Pfadzugriffe). Das ist der Grund, warum §4.2 Prüfung und Lesen in
   **einer** Umsetzung zusammenlegt.
8. **Keine Verzweigung in der Sicherheitslogik.** Ein Kommando mit rotem
   Risiko wird auf einem Server mit Schlüsseldatei-Anmeldung ausgeführt;
   Filter, Risikoeinstufung und Bestätigung verhalten sich **identisch**
   zu einem Server mit gespeichertem Schlüssel (5.6).

## 7. Umsetzungsreihenfolge

1. **Schnittstelle und Variante** in `core`: `KeyFileReader`,
   `AuthMethod::IdentityFile`, `resolve_auth` erweitern, die vier
   bestehenden Tests anpassen. Tests §6.1. *(opus — Credential-Handling)*
2. **Umsetzung des Lesens** samt Pfad-, Rechte- und Größenprüfung.
   Tests §6.2, §6.4.2, §6.4.3, §6.4.6, §6.4.7. *(opus)*
3. **Aufrufstelle** in `ssh-transport` (eine Zeile) und die Fehlermeldung
   je Hop (A-8). Tests §6.3.6. *(opus)*
4. **Persistenz und DTOs**: `AuthMethodInput::IdentityFile`, Rundlauf
   durch die JSON-Spalte, A-9. Tests §6.3.1, §6.3.7. *(opus)*
5. **Überführung** (Teil C) inklusive Rollback. Tests §6.3.3–5, §6.4.4,
   §6.4.5. *(opus — Credential-Handling)*
6. **Frontend**: Anmeldeart wählbar, Pfadfeld und Dateidialog, Vorab-
   Befund (B-3), Anzeige in Liste und Details, Überführungsdialog (C-2).
   i18n `de`/`en`. Tests §6.3.2. *(sonnet)*
7. **`CHANGELOG.md`** unter `[Unreleased]`. *(sonnet)*

Nach Schritt 4 ist Spec 0075 fahrbar; Schritt 5 und 6 können danach
laufen.

## 8. Offene Punkte

Keine. Die fünf Punkte, die diese Spec zur Entscheidung vorgelegt hat,
sind in §9 als E-1 bis E-5 entschieden; der Text oben ist bereits die
entschiedene Fassung.

## 9. Klarstellungen

**2026-09-23 · E-1 · Dateirechte** (→ A-4). Stefan: **genauso ablehnen
wie `ssh`**. Ist die Datei auf einem Unix-System für Gruppe oder Welt
lesbar, wird die Anmeldung abgelehnt; die Meldung nennt den
`chmod`-Befehl. Auf Windows entfällt die Prüfung, wie bei OpenSSH selbst.
Begründung: Es ist die einzige Fassung, die in `THREAT-MODEL.md`
(BL-0049) ohne Einschränkung aufschreibbar ist — „wir sind laxer als
`ssh`“ wäre bei diesem Produkt schwer zu vertreten.

**2026-09-23 · E-2 · Symbolische Links** (→ A-3). Stefan: **folgen**, wie
`ssh`. Ein Link auf einen Schlüssel auf einem verschlüsselten
Datenträger ist ein verbreitetes Muster. Wichtig für die Umsetzung: Die
Rechteprüfung aus E-1 greift auf der Datei, die **tatsächlich gelesen
wird**, nicht auf dem Link — deshalb Prüfung und Lesen auf demselben
offenen Dateihandle (§4.2, Test §6.4.7).

**2026-09-23 · E-3 · Passphrase** (→ A-5). Stefan: **optional im
Schlüsselbund, wie heute bei `PrivateKey`**. Gleiches Verhalten für beide
Schlüsselarten; die Wahl, sie nicht zu hinterlegen, hat der Nutzer
bereits.

**2026-09-23 · E-4 · Kein Löschangebot** (→ C-5). Stefan: Nach der
Überführung wird **nicht** angeboten, die Ursprungsdatei zu löschen. Der
Nutzer löscht selbst. Zeigt sich, dass die Datei regelmäßig vergessen
wird, ist das ein eigenes Item — nicht ein Knopf, der in seiner ersten
Fassung private Schlüssel löscht.

**2026-09-23 · E-5 · Lesen bei jedem Verbinden** (→ A-3, §4.3, K2, zur
Kenntnis vorgelegt, kein Widerspruch). Die Datei wird bei jedem
Verbindungsaufbau gelesen, nicht zwischengespeichert: Ein getauschter
Schlüssel wirkt sofort, und nichts liegt länger im Speicher als nötig.

*(Weitere Klarstellungen während der Umsetzung hier nachtragen: Datum ·
Frage-ID · Antwort.)*
