# Spec 0076 — Anmeldung mit einer Schlüsseldatei

Status: Vorschlag (Architekt, K3 entschieden) · Backlog: BL-0221, BL-0222 · Gate: release-1.0/D
Repo: **öffentlich** `smart-ssh` — `crates/core/src/ssh/auth.rs` und
`crates/core/src/profiles/types.rs` (Anmeldeart und Auflösung),
`crates/ssh-transport/` (Aufrufkette), `crates/app-shell/` (Dateizugriff,
Kommandos, DTOs, Zustand), Frontend
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

**Vorsicht bei dem, was diese Zahl bedeutet.** Sie sagt, wie oft
`resolve_auth` **gerufen** wird — nicht, wie groß der Eingriff ist. §4.2
gibt der Funktion einen dritten Parameter, und der muss durch die ganze
Kette geliefert werden, die ihn heute nirgends kennt: sechs Signaturen,
aufgezählt in §4.2. Die Aufrufstellen-Zahl ist also eine echte Messung
der falschen Größe; §4.2 und §7.3 messen die richtige.

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

**Eine Folge gehört benannt:** Eine Programmfassung, die eine
`auth_method`-Variante nicht kennt, kann den betroffenen Server nicht
lesen. Für die Fassung, die `IdentityFile` einführt, ist das
gegenstandslos — sie kennt die Variante. Für einen Rückschritt auf eine
**bereits ausgelieferte** Fassung hilft ohnehin nichts, die hat den Code
nie bekommen. Bleibt der Fall künftiger Varianten; er ist als **BL-0223**
festgehalten und ausdrücklich nicht Teil dieser Spec (§3.1, A-9).

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
5. **Keine neue Verzweigung auf die Anmeldeart im Transport.**
   `ssh-transport` bekommt dieselbe `ResolvedAuth::PrivateKey` wie heute
   (§4.1); die Anmeldelogik selbst bleibt unverändert. Angefasst wird
   dort nur, was der zusätzliche Parameter erzwingt (§4.2) und die
   Fehlermeldung je Hop (A-8).

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

- `~` am Anfang wird aufgelöst; ein relativer Pfad wird **abgelehnt**
  (Fehlermeldung), weil „relativ wozu" beim Verbinden keine beantwortbare
  Frage ist. **`~user` wird nicht unterstützt** — es bräuchte einen
  passwd-Lookup, den die Standardbibliothek nicht hergibt, gibt es auf
  Windows nicht, und keine Messung stützt eine Abhängigkeit dafür. Ein
  Pfad in dieser Form wird wie ein relativer behandelt: abgelehnt mit
  Begründung.
- Symbolischen Links wird gefolgt — `ssh` tut das auch (E-2).
- Die Datei wird **bei jedem Verbindungsaufbau** gelesen, nicht
  zwischengespeichert (E-5). Ein getauschter Schlüssel wirkt damit
  sofort, und nichts liegt länger im Speicher als nötig.
- **Nur reguläre Dateien** — und die Prüfung läuft auf **demselben
  offenen Handle** wie das Lesen, nicht auf einem Pfad davor. Unter Unix
  wird dafür mit `O_NONBLOCK` geöffnet (sonst blockiert ein benanntes
  Rohr, bis ein Schreiber erscheint) und anschließend per `fstat` auf dem
  Handle geprüft; ein Verzeichnis, ein Zeichengerät (`/dev/zero`,
  `/dev/stdin`), ein Rohr oder ein Socket wird dann abgelehnt. Auf
  Windows entfällt `O_NONBLOCK`; dort genügt die Prüfung auf dem Handle.

  Beides zusammen ist Absicht und nicht verhandelbar: Ein Vorab-`stat`
  auf den Pfad wäre ein Wettlauf (§6.4.7), ein Öffnen ohne `O_NONBLOCK`
  ein Hänger. Solche Objekte melden zudem Größe 0 und kämen durch jede
  Größenprüfung.
- Größe höchstens **1 MiB**, geprüft am `fstat` des Handles **und** beim
  Lesen: Es wird nie mehr als 1 MiB in den Speicher gelesen, auch wenn
  `fstat` weniger meldet. Die `fstat`-Prüfung ist die schnelle Abkürzung,
  die Lesegrenze ist die verbindliche.

**A-4 Dateirechte.** Ist die Datei auf einem Unix-System für Gruppe oder
Welt lesbar, wird die Anmeldung **abgelehnt** — mit derselben Begründung
wie `ssh` („unprotected private key file"), in eigener Formulierung und
mit dem `chmod`-Befehl, der es behebt. Auf Windows entfällt die Prüfung,
wie bei OpenSSH. Entschieden in E-1.

**A-4 gilt für die Anmeldung, nicht für den Befund und nicht für die
Überführung.** Der Vorab-Befund (B-3) und die Überführung (C-3) dürfen
eine Datei mit zu weiten Rechten lesen und **melden** den Mangel, statt
sie zu verweigern. Andernfalls wäre der Rettungsknopf aus C-1 genau dann
gesperrt, wenn er gebraucht wird — er behebt den Mangel ja (§4.5). Nach
der Überführung hängt die Anmeldung am Schlüsselbund, und A-4 ist
gegenstandslos.

**A-5** Die Passphrase wird wie bei `PrivateKey` behandelt: optional im
Schlüsselbund unter `passphrase_ref`. Ist keine hinterlegt und der
Schlüssel ist verschlüsselt, ergibt das eine Meldung, die genau das sagt
— nicht „Anmeldung fehlgeschlagen".

**Diese Entscheidung fällt in `resolve_auth`, nicht beim Lesen.** Ob eine
Passphrase hinterlegt ist, steht in `passphrase_ref` und wird über den
`CredentialStore` aufgelöst (`crates/core/src/ssh/auth.rs:57-61`); die
Datei weiß davon nichts. Die `KeyFileReader`-Umsetzung meldet deshalb
**nur**, ob der Schlüssel verschlüsselt ist; ob das ein Fehler ist,
entscheidet `resolve_auth` — das den Pfad aus `IdentityFile { path }`
kennt und ihn in der Meldung nennen kann. Ohne diese Trennung könnte ein
verschlüsselter Schlüssel **mit** hinterlegter Passphrase nie verbinden,
weil das Lesen schon mit einem Fehler endete.

**A-6 Fehlerpfade, jeder mit eigener Meldung**, und **keine** davon
enthält Dateiinhalt (§5.2):

| Fall | Meldung nennt |
|---|---|
| Datei fehlt | den Pfad, „nicht gefunden" |
| keine Leseberechtigung | den Pfad, „nicht lesbar" |
| Rechte zu weit (A-4) | den Pfad und den `chmod`-Befehl |
| größer als 1 MiB | den Pfad und die Grenze |
| kein gültiger OpenSSH-Schlüssel | den Pfad, „kein gültiger Schlüssel" |
| nicht als Text lesbar (kein UTF-8, NUL-Bytes) | den Pfad, „kein gültiger Schlüssel" |
| verschlüsselt, keine Passphrase hinterlegt | den Pfad, „Passphrase nötig" |
| Passphrase falsch | „Passphrase falsch" (ohne Pfad, s. u.) |
| relativer Pfad oder `~user` | den Pfad, „absoluter Pfad nötig" |
| kein reguläres Objekt (Verzeichnis, Gerät, Rohr) | den Pfad, „keine reguläre Datei" |

**Wer welche dieser Meldungen erzeugt, ist keine Geschmacksfrage**, denn
nur wer den Pfad kennt, kann ihn nennen. Nach A-2 gelangt in
`ssh-transport` bewusst nur `ResolvedAuth::PrivateKey { key, passphrase }`
— **ohne Pfad**; dort ist jede pfadtragende Meldung unmöglich
(`crates/ssh-transport/src/auth.rs:69-80` erzeugt heute genau diese
Fehler, pfadlos). Daraus die Aufteilung:

| Erzeugt von | Zeilen |
|---|---|
| `KeyFileReader`-Umsetzung (§4.2) | alles bis „kein gültiger Schlüssel", plus „keine reguläre Datei" und „absoluter Pfad nötig" |
| `resolve_auth` (kennt `path` **und** `passphrase_ref`) | „Passphrase nötig" (A-5) |
| `ssh-transport`, unverändert | „Passphrase falsch" — die einzige Zeile ohne Pfad |

Die Umsetzung prüft den Schlüssel also selbst auf **Gültigkeit** und
meldet, ob er **verschlüsselt** ist — sie entscheidet aber nicht, ob das
ein Fehler ist.

**A-7** Fehler beim Lesen sind `SshError::CredentialResolutionFailed`,
wie jeder andere Auflösungsfehler heute (`auth.rs:44-71`). Kein neuer
Fehlertyp, kein Panic.

**A-8 Die Anmeldeart gilt auch für Jump-Hosts.** `resolve_auth` läuft je
Hop (`ssh-transport/src/auth.rs:21`); ein Jump-Host mit Schlüsseldatei
funktioniert damit ohne Zusatzarbeit. Die Fehlermeldung MUSS sagen,
**welcher** Hop gescheitert ist.

**A-9 entfällt in dieser Spec — bewusst und benannt.** Ein früherer
Entwurf verlangte hier, dass eine unbekannte `auth_method`-Variante nicht
die ganze Serverliste mitreißt. Das ist richtig und wünschenswert, aber
es ist **nicht Gegenstand dieser Spec**: Weder BL-0221 noch BL-0222
verlangt es, und der Eingriff ist erheblich — heute bricht eine einzige
unlesbare Zeile das Laden aller Server ab
(`crates/persistence-sqlite/src/store.rs:354` mit `:290`), `Server.auth`
ist nicht-optional, `list_servers` gibt `ProfileResult<Vec<Server>>`
zurück (`crates/core/src/profiles/store.rs:71`, sechs Umsetzungen), und
das Löschen braucht ein `AuthMethod`, um die Schlüsselbund-Slots
abzuräumen (`crates/app-shell/src/server_credentials.rs:397-413`).

Für diese Spec ist es auch **nicht nötig**: Die Fassung, die
`IdentityFile` einführt, kennt die Variante. Betroffen wären nur
*künftige* Varianten, und dafür gibt es jetzt ein eigenes Item —
**BL-0223**.

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

Dafür braucht das Server-DTO ein **Ausgabefeld für den Pfad**. Heute
trägt es als einzigen Auth-Träger `auth_kind: AuthMethodKind`
(`crates/app-shell/src/dto.rs:49`), ein Enum ausdrücklich **ohne**
Inhalt (`:166-186`), weil dort nie ein Geheimnis hinaus soll
(`:36-38`). **Ein Dateipfad ist kein Geheimnis** — er darf hinaus, und
ohne ihn ist B-4 nicht umsetzbar. Das Feld ist leer, wenn die
Anmeldeart eine andere ist.

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
ist **nicht** Teil dieser Spec (E-4).

**C-6 Kein halber Zustand, in beide Richtungen.**

- Schlägt das **Schreiben in den Schlüsselbund** fehl, bleibt der Server
  unverändert auf `IdentityFile`.
- Schlägt danach das **Speichern des Servers** fehl, wird der eben
  geschriebene Schlüsselbund-Eintrag wieder entfernt. Sonst bliebe ein
  privater Schlüssel im Schlüsselbund liegen, auf den kein Server zeigt
  — genau der verwaiste Zustand, den diese Anforderung ausschließen
  soll.

Beide Richtungen sind das bestehende Muster, nicht eine Erfindung:
`crates/app-shell/src/servers.rs:25-32` beschreibt es für `create_server`
(alle Keychain-Slots werden abgeräumt, wenn der abschließende DB-Insert
scheitert), `:44-54` setzt es um.

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

Folge: `crates/ssh-transport/` bekommt **keine neue Verzweigung auf die
Anmeldeart**, und die Anmeldelogik selbst bleibt inhaltlich unverändert.
Die Signaturen wandern trotzdem — dazu §4.2, letzter Abschnitt. Eine eigene `ResolvedAuth::IdentityFile`-Variante wäre eine
zweite Fassung derselben Sache — verworfen.

### 4.2 Die Schnittstelle zum Dateisystem

`core` darf nicht ans Dateisystem (§1.3). Das Lesen bekommt deshalb eine
Schnittstelle, wie jede andere äußere Grenze in diesem Projekt
(`SshTransport`, `CredentialStore`, `ProfileStore`) — mit **zwei**
Operationen, weil drei Aufrufer zwei verschiedene Dinge brauchen:

```rust
// crates/core/src/ssh/auth.rs

/// Was eine Schlüsseldatei hergibt, ohne Urteil darüber, ob es reicht.
pub struct KeyFileContent {
    pub key: SecretString,
    pub encrypted: bool,
}

pub struct KeyFileFacts {
    pub exists: bool,
    pub permissions_too_open: bool,   // nur Unix, sonst immer false
    pub valid_key: bool,
    pub encrypted: bool,
    /// Der Grund, wenn die Datei gar nicht erst in Frage kommt —
    /// für die Meldung aus C-7 und B-3.
    pub problem: Option<KeyFileError>,
}

pub trait KeyFileReader {
    /// Öffnet die Datei und prüft Dateiart, Rechte und Größe auf
    /// **demselben** Handle (A-3), liest den Schlüssel und prüft ihn
    /// auf Gültigkeit. Alle pfadtragenden Fehler aus A-6 entstehen hier.
    ///
    /// `enforce_permissions`: `true` für die Anmeldung (A-4 lehnt ab),
    /// `false` für die Überführung und den Import — dort wird der
    /// Mangel gemeldet, nicht zur Sperre.
    fn read(&self, path: &str, enforce_permissions: bool)
        -> Result<KeyFileContent, KeyFileError>;

    /// Derselbe Weg, ohne den Schlüssel herauszugeben.
    fn inspect(&self, path: &str) -> KeyFileFacts;
}
```

`KeyFileError` hat **eine Variante je pfadtragender Zeile aus A-6** —
nicht eine Zeichenkette, gegen die §6.1.5 dann nur per Textvergleich
prüfen könnte:

`NotFound` · `NotReadable` · `PermissionsTooOpen` · `TooLarge` ·
`NotARegularFile` · `PathNotAbsolute` · `InvalidKey`. Jede trägt den
Pfad. A-7 bildet sie am Ende auf `SshError::CredentialResolutionFailed`
ab; unterscheidbar bleiben sie bis dorthin, damit §6.1.5 gegen Varianten
prüfen kann. **`PassphraseNoetig` ist bewusst keine Variante** — diese
Entscheidung fällt in `resolve_auth` (A-5).

**Warum `read` den Schalter `enforce_permissions` braucht:** A-4 lehnt
eine Datei mit zu weiten Rechten für die **Anmeldung** ab. Die
Überführung (C-3) und Spec 0075 Weg (b) müssen genau so eine Datei aber
**lesen** können — sie beheben den Mangel ja gerade, indem sie den
Schlüssel in den Schlüsselbund holen. Ohne den Schalter wäre der
Rettungsknopf aus C-1 genau dann gesperrt, wenn er gebraucht wird.

**Warum `inspect` eine eigene Operation ist:** B-3 (Vorab-Befund) und
C-7 (ist der Knopf wählbar?) brauchen dieselbe Feststellung — existiert
die Datei, wie sind die Rechte, ist es ein gültiger Schlüssel, ist er
verschlüsselt — **ohne** dass der Schlüssel herausgegeben wird. Ohne
benannte Operation baut jeder sie neu, mit eigenen Regeln.
(Spec 0075 Weg (b) benutzt dagegen `read`, denn dort wird der Inhalt
gebraucht.)

`resolve_auth` bekommt einen Parameter mehr:

```rust
pub fn resolve_auth(
    auth: &AuthMethod,
    credentials: &dyn CredentialStore,
    key_files: &dyn KeyFileReader,
) -> Result<ResolvedAuth, SshError>
```

**Und hier muss die Spec ehrlich sein.** Die Messung in §1.2 — „genau
eine Produktions-Aufrufstelle" — stimmt, misst aber die falsche Größe:
Ein zusätzlicher Parameter muss durch die **ganze Kette** geliefert
werden, die ihn heute nirgends kennt. Der `KeyFileReader` reist dabei
überall neben `credentials` mit, weil er dieselbe Herkunft und dieselbe
Lebensdauer hat. Das ist die Arbeitsliste für §7.1:

| Stelle | heute | was zu tun ist |
|---|---|---|
| `ssh-transport/src/auth.rs:16-21` | `authenticate(handle, hop, credentials)` | Parameter |
| `ssh-transport/src/connect.rs:130`, `:160` | rufen `authenticate` | durchreichen |
| `ssh-transport/src/connect.rs:83-87` | `connect(target, credentials, host_keys)` | Parameter |
| `app-shell/src/commands.rs:956-961` | ruft `ssh_transport::connect` **direkt**, nicht über den Trait | durchreichen |
| `app-shell/src/test_connection.rs:36-43` | Trait `Connector` | Parameter |
| `app-shell/src/test_connection.rs:48-57` | `RealConnector` | Parameter |
| `app-shell/src/test_connection.rs:412`, `:687` | zwei Mock-Umsetzungen | Parameter |
| `app-shell/src/state.rs:28-50`, `lib.rs:281-296` | `AppState` hat kein solches Feld | **neues Feld**, gebaut wie `credential_store` |
| `ssh-transport/tests/integration.rs` | rund zwei Dutzend `connect`-Aufrufe | anpassen, sonst ist `cargo test --workspace` rot |

Das sind sieben Signaturen, ein neues Zustandsfeld und die
Integrationstests — nicht „eine Zeile". Die Zeile
`app-shell/src/commands.rs:956` ist dabei der **echte** Produktionspfad;
der `Connector`-Trait ist die Testabstraktion daneben.

**Wer prüft was:** Die Umsetzung von `KeyFileReader` prüft alles am
Dateisystem — Pfadform, Dateiart, Rechte, Größe und Gültigkeit des
Schlüssels — auf **demselben offenen Handle** (A-3, §6.4.7). `core`
prüft nichts davon nach; es entscheidet nur, was daraus folgt (A-5).
Zwei Stellen mit halben Prüfungen sind schlimmer als eine mit ganzen.

Die konkrete Umsetzung liegt in `app-shell`; in Tests steht eine
Attrappe, mit der sich jeder Fehlerfall aus A-6 ohne echte Datei
herstellen lässt.

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
den Schlüssel in den Schlüsselbund — der Knopf ist auch dann wählbar,
wenn die Rechte zu weit liegen (A-4, letzter Absatz), denn er behebt
genau das. Genau dafür ist der Knopf da, und
genau so gehört er in der Oberfläche begründet.

## 5. Sicherheits-Invarianten

Berührt: **Credential-Handling** und **Ausführungspfad** (die Anmeldung
steht am Anfang jeder Verbindung). Daher `Review-Priorität: ERHÖHT`.

**5.1 Schlüsselmaterial bleibt in `SecretString`.** Der gelesene Inhalt
wird nie in eine Struktur mit abgeleitetem `Debug` und nie über eine
Grenze gereicht, die ihn protokolliert. `KeyFileReader::read` gibt
`SecretString` zurück, nicht `Vec<u8>`, damit am Übergang kein
ungeschützter Wert entsteht.

**Eine Einschränkung gehört dazu, sonst ist die Zusage falsch:** Eine
`SecretString` entsteht über einen `String`, also über eine
UTF-8-Prüfung — eine Zwischenform gibt es damit zwangsläufig, nur eine
kurzlebige und lokale. Inhalt, der kein gültiges UTF-8 ist (Binärdatei,
NUL-Bytes), scheitert schon dort; A-6 bildet das auf „kein gültiger
Schlüssel" ab. Nachgewiesen durch §6.4.1.

**5.2 Keine Fehlermeldung enthält Dateiinhalt.** Jede Meldung aus A-6
nennt **Pfad und Grund**, nie eine Zeile aus der Datei. Das betrifft
besonders „kein gültiger Schlüssel": Die Versuchung, den Anfang der Datei
mitzuliefern, ist genau der Fehler.

**Der Fehlertext der Parse-Bibliothek wird nicht durchgereicht**, sondern
durch eine eigene Meldung ersetzt. Heute wandert er wörtlich hinein
(`crates/ssh-transport/src/auth.rs:70`,
`format!("Private Key ungültig: {e}")`). Ob `ssh_key`s `Display` je
Eingabebytes wiedergibt, wissen wir nicht — und eine
Sicherheits-Invariante darf nicht auf dem Anzeigeverhalten einer fremden
Kiste ruhen. Eine eigene Meldung macht die Messung überflüssig.
Nachgewiesen durch §6.4.2.

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
5. Jeder Fehlerfall aus A-6 ergibt genau eine unterscheidbare
   `KeyFileError`-**Variante** (§4.2) — geprüft gegen die Variante, nicht
   gegen den Meldungstext. Ein Test gegen Zeichenketten wäre kaum zum
   Scheitern zu bringen und würde beim nächsten Umformulieren rot.

### 6.2 Dateizugriff (Umsetzung von `KeyFileReader`)

1. Absoluter Pfad, Rechte `0600`, gültiger Schlüssel → gelesen.
2. `~/…` wird aufgelöst; derselbe Schlüssel wird gefunden.
3. Relativer Pfad → Fehler „absoluter Pfad nötig" (A-3).
4. Rechte `0644` auf Unix → Fehler mit `chmod`-Befehl (A-4); auf Windows
   übersprungen.
5. Symbolischer Link auf eine gültige Datei → gefolgt (A-3).
6. Datei über 1 MiB → Fehler, und die Umsetzung liest sie **nicht**
   vollständig in den Speicher.
7. Verzeichnis, Zeichengerät und benanntes Rohr → je ein Fehler „keine
   reguläre Datei". Der Test auf das Rohr **darf nicht blockieren** —
   das ist der eigentliche Prüfpunkt, weil er `O_NONBLOCK` erzwingt
   (A-3). Ein Vorab-`stat` auf den Pfad würde hier zwar grün, aber
   §6.4.7 rot.
8. `~user/…` → Fehler „absoluter Pfad nötig" (A-3), nicht etwa ein
   Versuch, den Nutzer aufzulösen.
9. `inspect` liefert für dieselben Dateien denselben Befund wie `read`
   zurückmeldet, inklusive `problem` bei relativem Pfad, fehlender
   Datei, zu großer Datei und Nicht-Datei — und gibt **nie**
   Schlüsselmaterial heraus (§4.2).
10. `read(path, enforce_permissions: false)` liefert den Inhalt einer
   Datei mit Rechten `0644`; `read(path, true)` lehnt dieselbe Datei mit
   `PermissionsTooOpen` ab (§4.2, A-4).

### 6.3 Oberfläche und Überführung

1. Ein Server mit Schlüsseldatei lässt sich anlegen, speichern, neu laden
   und bearbeiten; die Anmeldeart überlebt den Rundlauf durch die
   Datenbank (JSON, §1.4).
2. Der Vorab-Befund aus B-3 meldet fehlende Datei, zu weite Rechte und
   „verschlüsselt" richtig — und hindert das Speichern nicht. Er benutzt
   `inspect`, nicht `read` (§4.2).
2a. Liste und Details zeigen den Pfad des Schlüssels; bei jeder anderen
   Anmeldeart ist das Feld leer (B-4). Der Test scheitert, solange das
   DTO den Pfad nicht trägt.
2b. Eine Datei mit Rechten `0644` lässt sich **überführen** (C-3) und der
   Knopf ist wählbar (C-7), obwohl A-4 die Anmeldung damit ablehnen
   würde. Das ist der Fall, für den es den Knopf gibt.
3. Überführung eines **unverschlüsselten** Schlüssels: Anmeldeart danach
   `PrivateKey`, Verbindung gegen die Testumgebung unverändert
   erfolgreich.
4. Überführung eines **passphrase-geschützten** Schlüssels: Der Inhalt im
   Schlüsselbund ist **byte-gleich** mit der Datei, die `passphrase_ref`
   ist mitgewandert, die Verbindung klappt (C-3, C-4).
5. Schreiben in den Schlüsselbund schlägt fehl → Server unverändert auf
   `IdentityFile` (C-6). **Und umgekehrt:** Schlüsselbund-Schreiben
   gelingt, das Speichern des Servers scheitert → der eben geschriebene
   Eintrag ist wieder weg, kein verwaister Schlüssel bleibt zurück.
6. Ein Jump-Host mit Schlüsseldatei verbindet; schlägt er fehl, nennt die
   Meldung **den Hop** (A-8).
7. **Falsche Passphrase** — Akzeptanzkriterium aus BL-0221: Ein
   verschlüsselter Schlüssel mit hinterlegter, aber falscher Passphrase
   ergibt die Meldung „Passphrase falsch"; sie enthält kein
   Schlüsselmaterial (A-6, 5.2).
8. **Verschlüsselter Schlüssel mit richtiger Passphrase verbindet.** Der
   Test scheitert, sobald das Lesen den Fall selbst als Fehler
   behandelt, statt ihn `resolve_auth` zu überlassen (A-5).

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

**Jeder Schritt muss für sich committbar sein und das Gate grün lassen**
(Repo-`CLAUDE.md`). Das bestimmt den Zuschnitt von Schritt 1 — und zwar
stärker, als es zunächst aussieht: Eine neue `AuthMethod`-Variante bricht
sofort jeden erschöpfenden `match` darüber, und der zusätzliche Parameter
von `resolve_auth` lässt sich nicht in `ssh-transport` einsetzen, ohne die
ganze Kette mitzuziehen (§4.2): `authenticate` kann sich keinen
`KeyFileReader` selbst bauen, die Umsetzung liegt in `app-shell`.

**Deshalb sind Variante, Schnittstelle und Kette ein einziger Schritt.**
Sie in zwei zu teilen ergäbe einen Zwischenstand, der nicht übersetzt.

1. **Variante, Schnittstelle, Kette — alles, was ohne einander nicht
   übersetzt.**
   - `AuthMethod::IdentityFile`, `KeyFileReader` mit `read`/`inspect`,
     `KeyFileContent`, `KeyFileFacts`, `KeyFileError`, `resolve_auth`
     erweitern (A-5: die Passphrase-Entscheidung fällt hier).
   - **Die drei erschöpfenden `match`-Ausdrücke über `AuthMethod`**
     mitziehen: `crates/app-shell/src/dto.rs:179`,
     `crates/app-shell/src/server_credentials.rs:401` und
     `crates/app-shell/src/server_credentials.rs:247-259`. Alle drei
     brechen laut — gut so.
   - **Und die eine Stelle, die still bricht:** das `matches!` in
     `crates/app-shell/src/server_credentials.rs:230-243` zählt vier
     Paare auf und hat einen impliziten `false`-Zweig. Ohne ein Paar
     `(IdentityFile, IdentityFile)` gilt beim Bearbeiten eines solchen
     Servers `same_kind == false` — und `update_server` löscht dann
     dessen `passphrase`-Slot aus dem Schlüsselbund. **Das ist ein
     Credential-Verlust ohne Fehlermeldung**, den kein Test aus §6
     fände. Diese Zeile ist der Grund, warum sie hier namentlich steht.
   - Die Kette aus der Tabelle in §4.2: sieben Signaturen, das neue
     `AppState`-Feld und die Aufrufe in
     `crates/ssh-transport/tests/integration.rs`.
   - Die vier bestehenden Tests in `core/src/ssh/tests.rs` anpassen.

   Tests §6.1, §6.4.8. *(opus — Credential-Handling, und der Schritt mit
   der größten Bruchfläche)*
2. **Umsetzung des Lesens**: `O_NONBLOCK`-Öffnen, `fstat` auf dem Handle,
   Dateiart, Rechte, Größe, Gültigkeit, dazu `inspect` und der Schalter
   `enforce_permissions`. Tests §6.2, §6.4.2, §6.4.3, §6.4.6, §6.4.7.
   *(opus)*
3. **Persistenz und DTOs**: `AuthMethodInput::IdentityFile`, das
   Ausgabefeld für den Pfad (B-4), Rundlauf durch die JSON-Spalte.
   Tests §6.3.1, §6.3.2a, §6.3.6–8. *(opus)*
4. **Überführung** (Teil C) inklusive Rollback in beide Richtungen.
   Tests §6.3.2b, §6.3.3–5, §6.4.4, §6.4.5. *(opus —
   Credential-Handling)*
5. **Frontend**: Anmeldeart wählbar, Pfadfeld und Dateidialog,
   Vorab-Befund (B-3), Anzeige in Liste und Details (B-4),
   Überführungsdialog (C-2). i18n `de`/`en`. Tests §6.3.2, §6.4.1.
   *(sonnet)*
6. **`CHANGELOG.md`** unter `[Unreleased]`. *(sonnet)*

**§6.4.1 steht bewusst in Schritt 5**, nicht bei der Leseumsetzung: Der
Test verlangt, dass Schlüsselmaterial nach einem Verbindungsaufbau weder
in der Datenbank noch in der Oberfläche auftaucht — beides gibt es vor
Schritt 3 bzw. 5 nicht.

Nach Schritt 3 ist Spec 0075 fahrbar; 4 und 5 können danach laufen.

## 8. Offene Punkte

Keine. Vier Punkte sind Stefan zur Entscheidung vorgelegt worden (E-1
bis E-4), einer zur Kenntnis (E-5, K2); alle fünf stehen in §9, und der
Text oben ist bereits die entschiedene Fassung. Ein Punkt wurde aus
dieser Spec **herausgelöst** statt entschieden: die Fehlertoleranz beim
Laden unbekannter Anmeldearten (früher A-9), jetzt **BL-0223** — siehe
§3.1 und §1.4.

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
