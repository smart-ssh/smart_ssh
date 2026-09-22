# Spec 0071 — Linux ohne Secret Service: klare Meldung statt englischem Backend-Text

Status: freigegeben (Stefan, 2026-09-22) · Backlog: BL-0031 · Gate: Release-Gate 1.0, A
Repo: **öffentlich** `smart_ssh` — `crates/credentials-keyring`,
`crates/app-shell` (`startup_error_messages.rs`, `lib.rs`, `error.rs`,
`dto.rs`, `server_credentials.rs` — dabei **alle** Startdialog-Texte,
auch die bestehenden aus Spec 0059, s. A11b), Frontend
(`apps/smart-ssh-community/frontend/src/locales/{de,en}/common.json`,
Diagnose-Ansicht)
Review-Priorität: **ERHÖHT** (Credential-Handling; adversariale Fälle in §6.3)

> Auf einer Debian-Minimal-Installation ohne Secret-Service-Anbieter sieht
> ein Nutzer heute den englischen Satz *„Credential-Backend-Fehler: No
> default store has been set, so cannot search or create entries"* — und
> zusätzlich einen Startdialog, der ihm sagt, alles außer dem Chat-Verlauf
> funktioniere normal. Beides ist unbrauchbar bzw. falsch: ohne
> Secret Service lässt sich **kein** KI-Provider und **kein**
> Server-Passwort anlegen. Diese Spec ersetzt beide Texte durch eine
> Meldung, die den Zustand benennt, die betroffenen Funktionen aufzählt und
> die konkreten Pakete nennt, die den Zustand beheben.

## Getroffene Entscheidungen (aus der Aufgabenstellung, Release-Gate 1.0 A)

- Es geht um eine **Meldung**, nicht um einen Ersatzspeicher. Smart SSH
  legt keine Secrets außerhalb des OS-Schlüsselbunds ab (s. §2, Nicht-Ziele
  und §8, OP-1).
- Zielumgebungen: Server, minimaler Desktop, headless-nahe Umgebungen.
- Prüfung des Gates: frische Debian-Minimal-VM ohne Keyring.

Die drei offenen Punkte in §8 sind entschieden (Stefan, 2026-09-22); die
Entscheidungen stehen dort und in §9 und sind in §2/§3 eingearbeitet.

---

## 1. Ausgangslage (belegt, Stand dieses Worktrees)

### 1.1 Wie der Schlüsselbund angebunden ist

`crates/credentials-keyring/src/lib.rs` implementiert `CredentialStore`
über `keyring` 4.1.6 im v1-Kompatibilitätsmodus (`keyring::Entry::new`,
`get_password`, `set_password`, `delete_credential`). Jeder Fehler außer
`NoEntry` wird pauschal zu `CredentialError::Backend(e.to_string())`
(`lib.rs:65`, `:81`, `:89`, `:102`).

`keyring` 4.1.6 initialisiert den Plattform-Store **einmalig und faul** in
einem `LazyLock` beim ersten `Entry::new` (`keyring-4.1.6/src/v1.rs`,
`SET_CREDENTIAL_STORE_RESULT`). Auf *nix ist das
`zbus_secret_service_keyring_store::Store::new()`, das über
`secret_service::blocking::SecretService::connect` läuft.

Daraus folgen zwei Dinge, die diese Spec ausnutzt:

1. **Schlägt die Initialisierung fehl, liefert `Entry::new` nur noch
   `Error::NoDefaultStore`** — die eigentliche Ursache ist dort bereits
   verloren. Ihr `Display`-Text lautet wörtlich: *„No default store has
   been set, so cannot search or create entries"*
   (`keyring-core-1.0.0/src/error.rs:105`).
2. **Die echte Ursache ist trotzdem abrufbar**, über
   `keyring::Entry::store_status() -> &'static Result<()>`
   (`keyring-4.1.6/src/v1.rs:66`). Der Aufruf ist beliebig oft billig: er
   liest denselben `LazyLock` und startet höchstens einmal einen
   Verbindungsversuch.

Die Ursachen, die `store_status()` auf Linux liefern kann
(`secret-service-5.1.0/src/{error,util}.rs`):

| Lage auf dem System | Fehler aus `secret-service` | `Display` |
|---|---|---|
| kein D-Bus-Session-Bus (kein `DBUS_SESSION_BUS_ADDRESS`, toter Socket) | `Error::Unavailable` (aus `zbus::Error::Address` / `InputOutput(NotFound)`, `util.rs:146`) | `no secret service provider or dbus session found` |
| Bus da, aber niemand besitzt `org.freedesktop.secrets` | `Error::Zbus(MethodError(ServiceUnknown …))` | `zbus error: …` |
| Anbieter da, aber gesperrt | `ServiceError::Locked` → `Error::NoStorageAccess` (`zbus-secret-service-keyring-store-1.0.1/src/errors.rs:13`) | `Couldn't access platform storage: …` |

Alle drei werden im Store zu `keyring::Error::PlatformFailure` bzw.
`NoStorageAccess` eingepackt (`errors.rs:25`, `:29`) — und beim späteren
`Entry::new` durch `NoDefaultStore` ersetzt.

### 1.2 Was der Nutzer heute sieht

**Beim Start** (`crates/app-shell/src/lib.rs:120–169`): der Chat-Inhalts-
Schlüssel wird aus dem Schlüsselbund aufgelöst. Scheitert das mit
`CipherError::KeyStoreAccessFailed` (`crypto/key.rs:40`, `:60`), zeigt
`startup_error_messages::keychain_unavailable_text` einen nicht-fatalen
Warndialog (Spec 0059, Fall 3). Dessen Text sagt heute wörtlich:

> „Chat-Verlauf, Notiz-Zusammenfassungen und die Eingabe-Historie sind für
> diesen Programmlauf deaktiviert. **Alle anderen Funktionen
> (SSH-Verbindungen, KI-Chat, Filter-Regeln) funktionieren normal** …"
> (`startup_error_messages.rs:153–157`)

und auf Linux zusätzlich einen Hinweis auf „gnome-keyring, KWallet oder
KeePassXC" mit der Frage, ob der Anbieter gesperrt sei
(`startup_error_messages.rs:158–164`).

**Diese Aussage ist im BL-0031-Fall falsch.** Ohne Secret Service scheitert
jeder Schreibzugriff auf den `CredentialStore`:

- `add_ai_provider` schreibt den API-Key **unbedingt** vor der DB-Zeile
  (`commands.rs:156–159`) — auch für Ollama mit leerem Key. Der Provider
  lässt sich also gar nicht anlegen; der Fünf-Minuten-Pfad endet hier.
- `update_ai_provider` (`commands.rs:203`), `delete_ai_provider`
  (`commands.rs:224`), Server-Passwort/Passphrase/Zertifikat
  (`server_credentials.rs:57`, `:101`, `:221`), Sudo-Passwort
  (`commands.rs:2398`).
- `state.credential_store.get(...)` für den aktiven Provider vor jedem
  KI-Aufruf (`commands.rs:747`, `:2569`).

Es funktionieren also **nur** SSH-Verbindungen mit Agent-Auth oder einem
Schlüssel ohne Passphrase sowie alles, was gar kein Secret braucht.

**In der Oberfläche**: `CommandError` entsteht über den blanket
`From<E: Display>`-Impl (`error.rs:69`); `code` bleibt `None`
(`error.rs:19–27`). Das Frontend zeigt daher den rohen, englischen
`Display`-Text aus §1.1 — unübersetzt und ohne jeden nächsten Schritt.

**Still falsch**: `ServerDto::from_server` setzt
`has_sudo_password: credential_store.get(...).is_ok()` (`dto.rs:85`). Ein
Backend-Fehler ist dort nicht von „kein Eintrag vorhanden" zu
unterscheiden — die Oberfläche behauptet dann „kein Sudo-Passwort
hinterlegt", obwohl sie es schlicht nicht weiß.

### 1.3 Gibt es wirklich einen Absturz?

Das Gate formuliert „statt Absturz". Ein `.expect()` auf dem
Schlüsselbund-Pfad ist im aktuellen Stand **nicht** mehr belegbar: Spec
0040 Abschnitt 7 hat es entfernt, der Fehler degradiert zu `None`
(`lib.rs:140–168`). Ob auf einer echten Debian-Minimal-VM trotzdem etwas
abbricht oder hängt (GTK ohne Session-Bus, `rfd`-Dialog, blockierender
`zbus`-Aufruf im Tokio-Worker), ist **nicht belegt** und deshalb Teil 0,
nicht Annahme.

---

## 2. Ziel und Nicht-Ziele

**Ziel:** Auf einem Linux-System ohne funktionierenden Secret Service
erfährt der Nutzer in seiner Sprache (a) *was* nicht geht, (b) *warum*, und
(c) *welches Paket* er installieren bzw. welchen Dienst er starten muss.
Kein englischer Bibliothekstext erreicht die Oberfläche.

**Nicht-Ziele** (jeweils bewusst, nicht vergessen):

1. **Kein Ersatz-Credential-Store** (verschlüsselte Datei, Klartextdatei,
   Prozessspeicher). Secrets bleiben im OS-Schlüsselbund. → §8, OP-1;
   weiterverfolgt als **BL-0203**.
2. **Kein Starten oder Installieren** von `gnome-keyring`, `kwalletd`,
   `dbus-user-session` durch Smart SSH. Die App gibt einen Befehl aus, sie
   führt ihn nicht aus.
3. **Keine Änderung am Verhalten auf macOS und Windows** außer: dort
   erscheinen weiterhin keine Linux-Paketnamen.
4. **Kein Wechsel der `keyring`-Version, kein Wechsel des Backends**
   (z. B. `linux-keyutils`), keine neue Abhängigkeit für D-Bus.
5. **Nicht BL-0117** (MCP-Bearer-Token im Klartext) und nicht BL-0089
   (endgültiger Keychain-Service-Name).
6. **Kein Freischalten des Fünf-Minuten-Pfads ohne Schlüsselbund.** Ob ein
   Ollama-Provider ohne API-Key künftig ohne Schlüsselbund anlegbar sein
   soll, ist eine eigene Entscheidung → §8, OP-2; weiterverfolgt als
   **BL-0204**.

---

## 3. Anforderungen

### Teil 0 — erst messen (kein Code)

**A0 (MUSS)** Der Coder richtet eine frische Debian-Minimal-VM **ohne**
Keyring ein, startet dort den aktuellen `main`-Build und berichtet in
`.agent/report.md`, **bevor** er etwas ändert:

- A0.1 Vollständige Ausgabe von `keyring::Entry::store_status()` inklusive
  der gesamten `source()`-Kette, jeweils in drei Lagen: (a) kein
  Session-Bus, (b) Session-Bus vorhanden, kein Secret-Service-Anbieter,
  (c) `gnome-keyring` installiert, aber gesperrt/nicht gestartet.
- A0.2 Startet die App, oder bricht sie ab? Wenn sie abbricht: wo genau,
  mit welchem Log? Erscheint der `rfd`-Warndialog aus Spec 0059 überhaupt?
- A0.3 **Exakte Paketnamen**, mit denen jede der drei Lagen auf Debian 13
  behoben wird — nachgewiesen durch `apt install` und einen anschließend
  erfolgreichen Provider-Anlegeversuch, nicht aus dem Gedächtnis. Mindestens
  zu prüfen: `gnome-keyring`, `dbus-user-session`, `libsecret-1-0`,
  KWallet (`kwalletmanager` bzw. `kwalletd6`), `keepassxc`. Ebenso:
  genügt die Installation, oder ist zusätzlich ein Dienststart/eine
  Anmeldesitzung nötig?
- A0.4 Hängt ein `CredentialStore`-Aufruf jemals länger als 2 s (toter
  Bus-Socket, Anbieter antwortet nicht)? Messwert angeben. Falls ja, ist
  das ein eigener Fund → neues Item, nicht in dieser Spec reparieren.

Der Bericht liegt vor, bevor gebaut wird.
Widerspricht ein Messwert dieser Spec, gilt der Messwert.

### Teil 1 — Ursache klassifizieren

**A1 (MUSS)** Eine neue, reine Klassifizierungsfunktion in
`crates/credentials-keyring` liefert aus dem Ergebnis von
`keyring::Entry::store_status()` einen `KeychainUnavailableReason` mit
genau diesen Varianten:

| Variante | Bedeutung |
|---|---|
| `NoSessionBus` | kein D-Bus-Session-Bus erreichbar (Linux) |
| `NoSecretServiceProvider` | Bus vorhanden, kein Anbieter unter `org.freedesktop.secrets` (Linux) |
| `Locked` | Anbieter vorhanden, aber gesperrt / Prompt abgewiesen |
| `Unknown` | alles andere, inklusive macOS/Windows |

**A2 (MUSS)** Die Funktion ist **ohne echten Schlüsselbund unit-testbar**:
Sie nimmt ihre Eingaben als Parameter entgegen (klassifizierter
Fehlergrund, Ziel-Betriebssystem als `&str`, Session-Bus-Indiz als
`bool`), niemals über `#[cfg(target_os)]` oder direkte
Umgebungs-/Dateisystemzugriffe. Vorbild und Begründung:
`startup_error_messages::keychain_unavailable_text` (`:146–151`).

**A3 (MUSS)** Das Session-Bus-Indiz wird vom Aufrufer ermittelt: gesetzt
und nicht leer ist `DBUS_SESSION_BUS_ADDRESS`, **oder**
`$XDG_RUNTIME_DIR/bus` existiert. Das Indiz steuert ausschließlich die
**Wortwahl** eines Textes — nie eine Sicherheitsentscheidung, nie ob ein
Secret geschrieben wird.

**A4 (MUSS)** `Unknown` ist der Auffangfall und muss einen vollständigen,
brauchbaren Text erzeugen. Eine Fehlklassifikation darf nie dazu führen,
dass gar keine Meldung erscheint.

### Teil 2 — Texte

**A5 (MUSS)** Für jede Variante aus A1 existiert ein Text, der drei Dinge
enthält: (a) den Zustand in einem Satz, (b) die Liste der **blockierten**
Funktionen, (c) genau einen nächsten Schritt mit konkretem Befehl.
Beispielhafte Zielform für `NoSecretServiceProvider` (Wortlaut Vorschlag,
Paketnamen nach A0.3 zu bestätigen):

> **Kein Systemschlüsselbund gefunden**
>
> Smart SSH speichert Passwörter, Passphrasen und API-Keys ausschließlich
> im Schlüsselbund des Betriebssystems. Auf diesem System läuft kein
> Secret-Service-Anbieter.
>
> Solange das so ist, lassen sich **keine KI-Provider, keine
> Server-Passwörter, keine Passphrasen und keine Sudo-Passwörter**
> speichern oder lesen. SSH-Verbindungen mit einem Schlüssel ohne
> Passphrase oder über den SSH-Agent funktionieren weiterhin.
>
> Nächster Schritt — einen Anbieter installieren und neu anmelden, z. B.:
> `sudo apt install gnome-keyring` (GNOME), `sudo apt install kwalletd6`
> (KDE) oder KeePassXC mit aktivierter Secret-Service-Integration.

**A6 (MUSS)** `NoSessionBus` nennt stattdessen den Session-Bus als Ursache
und `dbus-user-session` (bzw. das in A0.3 bestätigte Paket) als ersten
Schritt — **nicht** die Schlüsselbund-Pakete, die dort nicht helfen würden.

**A7 (MUSS)** `Locked` fordert zum **Entsperren** auf und nennt **kein**
zu installierendes Paket. Ein gesperrter Schlüsselbund darf nie als
fehlendes Paket dargestellt werden.

**A8 (MUSS)** Auf `macos` und `windows` enthält kein erzeugter Text einen
Linux-Paketnamen, keinen `apt`-Befehl und kein „Secret Service".

**A9 (MUSS)** Der bestehende Satz „Alle anderen Funktionen
(SSH-Verbindungen, KI-Chat, Filter-Regeln) funktionieren normal"
(`startup_error_messages.rs:155–156`) entfällt für die Fälle aus A1 und
wird durch die Aufzählung aus A5 (b) ersetzt. Der Satz „Smart SSH wird
jetzt trotzdem gestartet" bleibt — die App bricht weiterhin nicht ab.

**A10 (MUSS)** Kein Text enthält einen rohen Bibliotheksfehler, eine
D-Bus-Adresse, einen Benutzernamen oder einen Pfad, der nicht vorher durch
eine Steuerzeichen-Bereinigung analog `sanitize_path_for_display`
(`startup_error_messages.rs:31–37`) gelaufen ist.

**A11 (MUSS)** Alle Texte liegen in **DE und EN** vor. Für Frontend-Texte
gilt: neue Schlüssel in `locales/de/common.json` **und**
`locales/en/common.json`.

**A11a (MUSS, Entscheidung OP-3 (c), 2026-09-22)** Der Startdialog läuft
vor der Tauri-Runtime und damit vor der Frontend-`i18n`. Er wählt seine
Sprache aus der **Systemumgebung**: erste gesetzte, nicht leere Variable
aus `LC_ALL`, `LC_MESSAGES`, `LANG`; ausgewertet wird nur das
Sprach-Präfix vor `_`, `.` oder `@` (`de_DE.UTF-8` → `de`). Beginnt es mit
`de` → Deutsch, sonst **Englisch**. Ist keine Variable gesetzt oder ist
der Wert unbrauchbar (`C`, `POSIX`, leer), gilt **Deutsch** als Vorgabe —
dasselbe Verhalten wie heute.

**A11b (MUSS)** Die Sprachwahl gilt für **alle** Startdialog-Texte, nicht
nur für die neuen: Ein englischsprachiges System darf nicht bei einem
DB-Fehler Deutsch und bei einem Schlüsselbund-Fehler Englisch zeigen.
Deshalb bekommen auch die bestehenden Spec-0059-Texte
(`db_connect_failure_text` mit allen drei `ConnectFailureKind`,
`host_key_store_failure_text`, `keychain_unavailable_text`) eine
EN-Fassung. Inhaltlich sind die EN-Fassungen Übersetzungen, keine
Neufassungen — insbesondere bleibt die Warnung in
`host_key_store_failure_text`, dass eine erneute Erstbestätigung **keinen**
Schutz vor einem untergeschobenen Server bietet, in beiden Sprachen
gleich deutlich.

**A11c (MUSS)** Die Sprachwahl ist eine **reine Funktion** über den
Umgebungswert (`fn startup_language(raw: Option<&str>) -> Language`),
unit-testbar ohne Umgebungsmanipulation — dieselbe Parameter-Injection
wie bei `keychain_unavailable_text(target_os)`
(`startup_error_messages.rs:146–151`). Nur der Aufrufer in `lib.rs` liest
die Variablen tatsächlich aus.

### Teil 3 — Verdrahtung

**A12 (MUSS)** Der Startdialog (`lib.rs:159–166`) ruft die neue Textwahl
mit dem klassifizierten Grund auf statt des bisherigen pauschalen
`keychain_unavailable_text(OS)`.

**A13 (MUSS)** `CredentialError::Backend` aus einem nicht verfügbaren
Store erreicht das Frontend mit dem stabilen Code
`KEYCHAIN_UNAVAILABLE` über `CommandError::with_code`
(`error.rs:38–44`, Konvention aus Spec 0024, Abschnitt 5). Das Frontend
zeigt daraufhin den übersetzten Text aus A5, nicht `message`. Der Code
wird in `code_tests::test_command_error_with_code_values_are_unique`
(`error.rs:86–104`) mit aufgenommen.

**A14 (MUSS)** `ServerDto::has_sudo_password` unterscheidet „nicht
vorhanden" (`CredentialError::NotFound` → `false`) von „Backend nicht
verfügbar". Im zweiten Fall darf das Feld **nicht** `false` behaupten;
das DTO erhält dafür ein zusätzliches, serialisiertes Feld (Vorschlag:
`sudo_password_unknown: bool`), und die Oberfläche zeigt statt „kein
Sudo-Passwort hinterlegt" einen neutralen Zustand.

**A15 (SOLL)** Die Diagnose-Ansicht (`settings.categories.diagnostics`)
zeigt eine Zeile „Systemschlüsselbund: verfügbar / nicht verfügbar
(Grund)". Damit ist der Zustand auch dann nachschlagbar, wenn der
Startdialog weggeklickt wurde.

**A16 (MUSS)** Der Zustand wird **einmal pro Programmlauf** ermittelt und
im `AppState` gehalten. Kein Kommando probiert den Schlüsselbund
zusätzlich ab, um den Zustand zu erfahren. (`keyring` hält seinen
Initialisierungsversuch ohnehin in einem `LazyLock` — A16 verhindert
zusätzliche eigene Probiervorgänge, insbesondere auf dem
`list_servers`-Pfad, der `from_server` pro Server aufruft.)

**A17 (MUSS, Entscheidung 2026-09-22)** Ein vom Nutzer **ausgelöstes**
Entfernen eines Secrets meldet nie Erfolg, wenn das Secret nicht entfernt
werden konnte:

- **„Hinterlegtes Sudo-Passwort entfernen"** (`clear_sudo_password`)
  schlägt sichtbar fehl. Es ist sonst nichts geschehen, das sich melden
  ließe — ein Erfolgssignal wäre schlicht unwahr. Der Fehler nutzt
  denselben Weg wie A13 (`KEYCHAIN_UNAVAILABLE`, übersetzt).
- **Server löschen** (`delete_auth_method_secrets`) läuft **durch**: Das
  Profil wird gelöscht, auch wenn das Secret bleibt. Ein Nutzer darf nicht
  auf einem unlöschbaren Server sitzen bleiben, nur weil der Schlüsselbund
  klemmt. Das Ergebnis sagt aber ausdrücklich, dass das Secret nicht
  entfernt werden konnte und im Schlüsselbund zurückbleibt — mit dem
  Hinweis, dass der Eintrag dort dann verwaist ist (die Server-ID
  existiert nicht mehr) und von Hand gelöscht werden kann.
- In beiden Fällen bleibt die bereits vorhandene `tracing::warn!`-Zeile.
- Test: Für beide Pfade ein Fall mit einem Store, der `Backend` liefert —
  `clear_sudo_password` gibt einen Fehler zurück, das Server-Löschen
  gelingt und meldet den Rückstand. Beide scheitern gegen den Stand von
  heute, weil dort `let _ = …` steht bzw. nur geloggt wird.

---

## 4. Design

### 4.1 Wo die Klassifizierung liegt

In `crates/credentials-keyring`, nicht in `app-shell`: dort liegt bereits
das gesamte Wissen über die `keyring`-Crate, und `app-shell` „enthält keine
fachliche Logik" (Spec 0007, Abschnitt 3; s. Modul-Kommentar in
`credentials-keyring/src/lib.rs:9–17`). `app-shell` bekommt nur den
fertigen `KeychainUnavailableReason` und wählt daraus den Text — genau die
Trennung, die `startup_error_messages` bereits gegenüber `startup_dialog`
zieht.

### 4.2 Warum `store_status()` und nicht der Fehler aus `Entry::new`

Weil `Entry::new` die Ursache bereits verworfen hat (§1.1, Punkt 1).
`store_status()` ist die einzige Stelle, an der der ursprüngliche Fehler
noch existiert, und sie ist vom `keyring`-Autor genau dafür vorgesehen
(Doc-Kommentar `v1.rs:56–65`: *„if you want to do runtime checking of
credential store initialization without first creating an entry"*).
Verworfen: den Fehlertext aus `Entry::new` per String-Vergleich deuten —
er enthält die Ursache gar nicht.

### 4.3 Wie `NoSessionBus` von `NoSecretServiceProvider` getrennt wird

Über das Umgebungsindiz aus A3, nicht über den Wortlaut des
`zbus`-Fehlers. Begründung: Der Unterschied besteht im Fehlerobjekt nur
zwischen `secret_service::Error::Unavailable` und einem
`zbus::Error::MethodError` — beides landet im Store in derselben
`keyring::Error::PlatformFailure`-Hülle (`errors.rs:25`), sodass am
`keyring`-API nur noch ein `String` ankommt. Ein String-Vergleich auf
fremde Fehlertexte bricht beim nächsten Versionssprung still.

Das Umgebungsindiz ist eine **Heuristik** und wird in der Spec auch so
behandelt: Sie darf nur die Wortwahl steuern (A3). Liegt sie falsch, nennt
der Text das falsche der beiden Pakete — ärgerlich, aber folgenlos für die
Sicherheit; beide Texte enden mit demselben Hinweis, dass danach eine neue
Anmeldesitzung nötig ist.

Verworfene Alternative: ein einziger Text für beide Fälle, der Session-Bus
und Schlüsselbund-Anbieter gemeinsam aufzählt. Er wäre einfacher, träfe
aber genau die Unschärfe, die BL-0031 beanstandet („welches Paket fehlt").
Falls Teil 0 zeigt, dass das Indiz auf den Zielsystemen unzuverlässig ist,
fällt die Umsetzung auf diese Variante zurück — dann mit Vermerk in §9.

### 4.4 Fehlerpfade

| Wo es scheitert | Was der Nutzer sieht |
|---|---|
| Start, Chat-Schlüssel nicht auflösbar | nicht-fataler `rfd`-Warndialog mit dem Text nach A5/A6/A7; App startet |
| Provider anlegen/ändern/löschen | Fehler im Provider-Dialog, übersetzter Text über `KEYCHAIN_UNAVAILABLE` |
| Server-Passwort/Passphrase speichern | Fehler im Server-Formular, gleicher Text |
| Sudo-Passwort setzen | Fehler im Sudo-Dialog, gleicher Text |
| Aktiven Provider für einen KI-Aufruf lesen | Fehler im Chat, gleicher Text |
| Zustand nachschlagen | Diagnose-Ansicht, A15 |

### 4.5 Was ausdrücklich unverändert bleibt

Die Degradierung aus Spec 0040 Abschnitt 7 (Chat-Persistenz,
Prompt-Historie, Ledger werden `None`) und die Entscheidung aus Spec 0059,
dass ein Schlüsselbund-Problem den Start **nicht** abbricht. Diese Spec
ändert nur, *was* gemeldet wird, nicht *ob* gestartet wird.

---

## 5. Sicherheits-Invarianten

**Berührt (Credential-Handling):**

- **I1 — Kein Secret in einer Meldung.** Keine der neuen Funktionen nimmt
  einen `SecretString`, einen `CredentialRef`-Wert oder einen rohen
  Backend-Fehler entgegen; sie nehmen nur `KeychainUnavailableReason`,
  `&str` (Ziel-OS) und `bool`. Dieselbe Bauweise wie
  `startup_error_messages` (dortiger Modul-Kommentar, `:8–12`).
- **I2 — Kein stiller Rückfall.** Ist der Schlüsselbund nicht verfügbar,
  scheitert jeder Schreibzugriff sichtbar. Es entsteht kein zweiter
  Speicherort und kein „merke es dir für diese Sitzung".
- **I3 — Kein falsches Erfolgssignal.** Ein fehlgeschlagener
  Credential-Schreibzugriff führt nie zu einem angelegten Profil ohne
  Secret; die bestehende Rückbau-Reihenfolge in `add_ai_provider`
  (`commands.rs:160–172`) bleibt unverändert.
- **I4 — Unbekannt ist nicht „nein".** A14: Ein Backend-Fehler wird nie als
  „kein Secret hinterlegt" dargestellt.
- **I5 — Keine Empfehlung, die Sicherheit senkt.** Kein Text schlägt vor,
  Secrets in einer Datei, einer Umgebungsvariable oder einem
  passwortlosen Schlüsselbund abzulegen. Wird ein leeres Passwort für den
  Login-Keyring erwähnt, dann nur als ausdrückliche Warnung.

**Nicht berührt:** Filter-Engine, Risiko-Klassifizierer, Redactor,
Ausführungspfad (Confirm/AutoExec), Verschlüsselungsalgorithmen,
Migrationen. Diese Spec ändert keine Entscheidung darüber, *ob* etwas
ausgeführt wird — nur Meldungstexte, einen Fehlercode und ein DTO-Feld.

---

## 6. Tests

### 6.1 Unit-Tests: Klassifizierung (`credentials-keyring`)

- T1 `Unavailable`-artiger Grund **ohne** Session-Bus-Indiz →
  `NoSessionBus`.
- T2 Derselbe Grund **mit** Session-Bus-Indiz →
  `NoSecretServiceProvider`.
- T3 `NoStorageAccess`-artiger Grund → `Locked`, unabhängig vom Indiz.
- T4 Erfolgreicher `store_status()` → gar keine Meldung (kein Dialog).
- T5 Ziel-OS `macos`/`windows` → `Unknown`, unabhängig vom Indiz.

Jeder Test scheitert am ungefixten Stand, weil die Funktion dort nicht
existiert; nach dem Bau scheitert T3 an jeder Implementierung, die
„gesperrt" mit „nicht installiert" verwechselt.

### 6.2 Unit-Tests: Texte (`startup_error_messages`)

Nach dem Muster der bestehenden Tests (`:286–309`):

- T6 `NoSecretServiceProvider` (linux) nennt alle drei Anbieter und
  mindestens einen Installationsbefehl.
- T7 `NoSessionBus` (linux) nennt den Session-Bus und **nicht**
  `gnome-keyring` als ersten Schritt.
- T8 `Locked` (linux) enthält **kein** `apt`/`install`.
- T9 `macos` und `windows` enthalten weder `gnome-keyring` noch `apt`
  noch „Secret Service".
- T10 Jeder Text enthält die Aufzählung der blockierten Funktionen
  (Prüfung auf „API", „Passwort", „Passphrase") und **nicht** mehr den Satz
  „funktionieren normal".
- T11 Jeder Text enthält „trotzdem gestartet" (Spec 0059,
  Nicht-Fatalität bleibt).
- T11a Sprachwahl (A11c): `de_DE.UTF-8`, `de`, `de_AT@euro` → Deutsch;
  `en_US.UTF-8`, `fr_FR`, `ja_JP.UTF-8` → Englisch; `C`, `POSIX`, `""`,
  `None` → Deutsch (Vorgabe).
- T11b Für **jeden** Startdialog-Text existiert beides — DE und EN — und
  die beiden Fassungen sind nicht identisch. Ein Test über alle Fälle
  (A11b), damit eine vergessene Übersetzung nicht still als
  deutsche Zeichenkette durchrutscht.
- T11c Die EN-Fassung von `host_key_store_failure_text` enthält die
  „kein Schutz"-Warnung ebenso ausdrücklich wie die deutsche (Analogon zu
  `test_host_key_store_failure_message_warns_about_deleting_the_file`,
  `:248`).
- T12 Der bestehende Test
  `test_no_generated_text_leaks_a_placeholder_secret_value` (`:318`) wird
  um alle neuen Texte erweitert.

### 6.3 Adversariale Fälle (Review-Priorität ERHÖHT)

- **X1 — Steuerzeichen aus der Umgebung.** Ein
  `DBUS_SESSION_BUS_ADDRESS` mit `\n`/`\r` (bzw. jeder Wert, der in einen
  Text gelangt) darf den Dialogtext nicht optisch fortsetzen — Analogon zu
  `test_sanitize_path_for_display_replaces_control_characters` (`:260`).
  Bevorzugt wird der Wert gar nicht ausgegeben; wird er ausgegeben, gilt
  A10.
- **X2 — Kein Secret über die Fehlerkette.** Ein konstruierter
  Backend-Fehler, dessen `Display` einen Platzhalter wie
  `sk-live-hunter2` enthält, darf in **keinem** erzeugten Text und in
  **keiner** `CommandError.message` mit Code `KEYCHAIN_UNAVAILABLE`
  auftauchen. Der Code ersetzt den Text, er ergänzt ihn nicht.
- **X3 — Gesperrt ≠ fehlt.** Ein gesperrter, vorhandener Schlüsselbund darf
  den Nutzer nie zu `apt install` schicken (T3 + T8 zusammen). Sonst
  installiert ein KDE-Nutzer `gnome-keyring` neben sein laufendes KWallet
  und verschlimmert den Zustand.
- **X4 — „Unbekannt" wird nicht zu „nein".** Ein Server mit hinterlegtem
  Sudo-Passwort und einem Store, der `Backend` liefert, meldet **nicht**
  `has_sudo_password: false` (A14). Am ungefixten Stand schlägt dieser
  Test fehl, weil `dto.rs:85` `.is_ok()` verwendet.
- **X5 — Kein unsicherer Rat.** Kein erzeugter Text enthält Formulierungen,
  die zu einem Klartext-Ablageort oder einem passwortlosen Schlüsselbund
  raten. Als exekutierbarer Test: keine der Zeichenketten „Klartext",
  „plain text", „ohne Passwort", „empty password" ohne begleitendes
  Warnwort.
- **X6 — Kein Umweg um die Meldung.** Es gibt keinen Codepfad, der bei
  nicht verfügbarem Store trotzdem einen Erfolg zurückgibt: Alle
  `credential_store.set(...)`-Aufrufe reichen ihren Fehler weiter (heute:
  `commands.rs:158`, `:203`, `server_credentials.rs:57`, `:101`, `:221`).

  **Korrektur vom 2026-09-22 (vorige Fassung dieser Spec war hier
  falsch):** Nicht jeder verschluckte `delete` ist ein Aufräumpfad. Zu
  trennen sind zwei Sorten:

  - **Echte Aufräumpfade** — ein `delete`, das einen bereits
    fehlgeschlagenen Vorgang zurückbaut (`commands.rs:170`:
    Rückbau nach einem DB-Fehler in `add_ai_provider`;
    `server_credentials.rs:156`: aufgegebene Slots beim Wechsel der
    Auth-Methode). Hier bleibt der Fehler verschluckt, weil der
    eigentliche Fehler die relevante Information ist. Im Review einzeln
    als solche zu bestätigen.
  - **Vom Nutzer ausgelöstes Entfernen** — `clear_sudo_password`
    (`server_credentials.rs:73`) und `delete_auth_method_secrets`
    (`:268`ff.) beim Löschen eines Servers. Das ist **kein** Aufräumen:
    Der Nutzer hat das Entfernen angefordert. Ein verschluckter Fehler
    meldet hier Erfolg, während das Secret im Schlüsselbund stehen
    bleibt. Siehe A17.

### 6.4 Manuelle Tests (in `manuelle-tests-ausstehend.md` übernehmen)

- M1 Debian-Minimal-VM, kein Keyring, kein Session-Bus: App starten →
  Dialogtext nach A6, App läuft, Log enthält den vollen Grund.
- M2 Dieselbe VM mit `dbus-user-session`, ohne Anbieter → Dialogtext nach
  A5. Anschließend Provider anlegen → übersetzter Fehler, kein englischer
  Bibliothekstext.
- M3 `gnome-keyring` nach A0.3 installieren, neu anmelden → Provider
  anlegen gelingt, kein Dialog mehr, Diagnose-Zeile „verfügbar".
- M4 KDE-VM mit gesperrtem KWallet → Text nach A7, kein `apt`-Befehl.
- **M1–M4 zusätzlich (Q-BL-0031-01, Stefan 2026-09-22):** In jeder Lage
  wird geprüft, ob der angezeigte Text **das Paket nennt, das die Lage
  tatsächlich behebt** — also: den genannten Befehl ausführen, neu
  anmelden, Provider anlegen. Gelingt das nicht, ist der Paketname falsch
  und `ANNAHME A-1` gilt als widerlegt. Dabei mit zu erheben, was A0.1
  (a/b) und A0.4 gemessen hätten: die volle `store_status()`-Fehlerkette
  und die Dauer eines `CredentialStore`-Aufrufs.
- M5 macOS mit abgebrochenem Keychain-Dialog → unverändertes Verhalten,
  keine Linux-Paketnamen.
- M6 Dieselbe VM einmal mit `LANG=en_US.UTF-8` und einmal mit
  `LANG=de_DE.UTF-8` starten → derselbe Dialog in der jeweiligen Sprache;
  zusätzlich ein künstlich herbeigeführter DB-Fehler (Spec 0059, Fall 2)
  unter `en_US.UTF-8` → ebenfalls englisch, nicht deutsch (A11b).

---

## 7. Umsetzungsreihenfolge (einzeln committbar)

1. **Teil 0**: Bericht nach A0 in `.agent/report.md`. Kein Code.
2. `KeychainUnavailableReason` + Klassifizierung in `credentials-keyring`,
   mit T1–T5. Keine Verdrahtung.
3. Sprachwahl `startup_language` (A11a/A11c) als reine Funktion, mit
   T11a. Noch ohne Wirkung.
4. Texte in `startup_error_messages`: neue Texte (A5–A9) **und**
   EN-Fassungen der bestehenden Spec-0059-Texte (A11b), mit T6–T12 und
   X1/X5.
5. Startdialog auf Sprachwahl und neue Texte umstellen (`lib.rs`),
   Zustand in `AppState` (A16).
6. `KEYCHAIN_UNAVAILABLE` in `error.rs`, Mapping an den
   `CredentialStore`-Aufrufstellen, Frontend-Schlüssel DE/EN, mit X2.
7. `has_sudo_password`/`sudo_password_unknown` (A14), mit X4.
8. Diagnose-Zeile (A15).
9. Manuelle Tests M1–M6 auf der VM, Ergebnis in `.agent/report.md`.

Reihenfolge-Begründung: Schritte 2 bis 4 sind reine, vollständig
getestete Funktionen ohne Wirkung; erst Schritt 5 ändert sichtbares
Verhalten. So bleibt jeder Commit einzeln prüfbar und rückbaubar.

**Konflikthinweis:** BL-0013 (Spec 0069) ist noch nicht gemergt und
ändert `locales/de/common.json` und `locales/en/common.json` (je ~50
Zeilen). Diese Spec fasst dieselben Dateien an. Entweder BL-0013 zuerst
mergen und diesen Worktree danach auf `main` rebasen, oder in Schritt 5
mit einem Konflikt in genau diesen zwei Dateien rechnen.

---

## 8. Offene Punkte — **alle drei entschieden (Stefan, 2026-09-22)**

Die Optionen bleiben als Begründung stehen; die getroffene Entscheidung
steht jeweils darunter und ist oben in §2/§3 eingearbeitet.

### OP-1 — Ersatzspeicher, wenn kein Schlüsselbund existiert?

Auf einer Debian-Minimal-VM ohne Anbieter ist Smart SSH auch nach dieser
Spec **nicht benutzbar** für alles, was ein Secret braucht — es gibt dann
eben eine gute Erklärung statt einer schlechten.

- **(a) Nichts (Empfehlung).** Secrets bleiben ausschließlich im
  OS-Schlüsselbund. Das ist die Zusage, die Threat Model und Website tragen
  werden; ein datei-basierter Ersatz müsste mit einem Passwort geschützt
  werden, das Smart SSH selbst abfragt und verwaltet — ein eigenes,
  sicherheitskritisches Vorhaben, kein Nebenprodukt von BL-0031.
- **(b) Verschlüsselte Datei mit Master-Passwort**, opt-in, deutlich
  gekennzeichnet. Macht die App auf Servern und minimalen Desktops
  nutzbar. Umfang: eigene Spec, eigener Review, neuer Angriffsvektor.
- **Folge bei (a):** Ein Teil der Linux-Zielgruppe („Homelab, Server")
  kann die App ohne Zusatzinstallation nicht nutzen. Das gehört dann
  in README und Website unter Systemvoraussetzungen (eigenes Item).

> **Entscheidung (Stefan, 2026-09-22): (a)** — vorerst passiert nichts.
> Der verschlüsselte Datei-Store kommt „irgendwann" und ist als
> **BL-0203** weiterverfolgt. Diese Spec bleibt bei
> Nicht-Ziel 1.

### OP-2 — Ollama ohne API-Key: Provider ohne Schlüsselbund anlegbar?

`add_ai_provider` schreibt den Credential **unbedingt** (`commands.rs:156`)
— auch bei leerem `api_key`. Dadurch scheitert auf einer VM ohne
Schlüsselbund selbst der lokale Ollama-Pfad, obwohl dort gar kein Secret
zu schützen wäre. Das trifft direkt den Fünf-Minuten-Pfad (BL-0035).

- **(a) Separates Item (Empfehlung).** BL-0031 bleibt bei der Meldung;
  „leerer Key schreibt kein Credential" wird ein eigenes Item mit eigener
  Prüfung, weil es das Verhalten auf **allen** Plattformen ändert und den
  Provider-Anlegepfad berührt.
- **(b) Hier miterledigen.** Ein Schritt mehr in §7, dafür ist der
  Fünf-Minuten-Pfad mit Ollama auf der Debian-Minimal-VM sofort begehbar.
- **Zu bedenken:** Bei (b) muss der Lesepfad (`commands.rs:747`, `:2569`)
  ein fehlendes Credential als „kein Key nötig" behandeln — das ist genau
  die Sorte Lockerung, die laut Architektur-Checkliste einen eigenen,
  adversarialen Review verdient.

> **Entscheidung (Stefan, 2026-09-22): (a)** — eigenes Item. Angelegt als
> **BL-0204**. Diese Spec bleibt bei
> Nicht-Ziel 6.

### OP-3 — Sprache des Startdialogs

Der Startdialog läuft vor der Tauri-Runtime und damit vor jeder
Sprachwahl; seine Texte sind heute fest deutsch
(`startup_error_messages.rs`). BL-0112/BL-0111 zeigen, dass das ein
bekanntes, offenes Muster ist.

- **(a) Wie bisher: Deutsch (Empfehlung für diese Spec).** Konsistent mit
  allen anderen Spec-0059-Texten; die Sprachfrage wird einmal für alle
  Startdialoge entschieden, nicht hier nebenbei.
- **(b) Zweisprachig im selben Dialog** (DE-Absatz, dann EN-Absatz). Löst
  das Problem für internationale Tester sofort, macht den Dialog aber
  doppelt so lang.
- **(c) Sprache aus der Systemumgebung** (`LANG`/`LC_MESSAGES`) wählen.
  Sauberste Lösung, gehört aber zu einem eigenen Item für alle
  Startdialoge.

> **Entscheidung (Stefan, 2026-09-22): (c)** — Sprachwahl aus
> `LC_ALL`/`LC_MESSAGES`/`LANG`, Vorgabe Deutsch. Eingearbeitet als
> A11a–A11c, T11a–T11c, §7 Schritte 3/4 und M6.
>
> **Folge für den Umfang, ausdrücklich benannt:** Damit kein System
> gemischte Sprachen zeigt, bekommen auch die **bestehenden**
> Spec-0059-Texte eine EN-Fassung (A11b). Das sind vier weitere Texte,
> die diese Spec sonst nicht angefasst hätte, und sie liegen auf einem
> Pfad mit einer Sicherheitswarnung (Host-Key-Speicher) — deshalb T11c.
> Der Schritt ersetzt ein eigenes Item für die Startdialog-Sprache;
> BL-0111 (englischer System-Prompt) und BL-0112 (Dateibrowser-i18n)
> bleiben davon unberührt.

---

## 9. Klarstellungen

- **2026-09-22 · OP-1 · Stefan:** Kein Ersatzspeicher. Secrets bleiben
  ausschließlich im OS-Schlüsselbund; der verschlüsselte Datei-Store wird
  als BL-0203 verfolgt.
- **2026-09-22 · OP-2 · Stefan:** „Leerer API-Key schreibt kein
  Credential" wird nicht hier miterledigt, sondern als BL-0204 geführt.
- **2026-09-22 · OP-3 · Stefan:** Der Startdialog wählt seine Sprache aus
  `LC_ALL`/`LC_MESSAGES`/`LANG` (Vorgabe Deutsch); die Wahl gilt für alle
  Startdialog-Texte, auch die bestehenden aus Spec 0059.

- **2026-09-22 · Teil 0, Aufteilung:** Teil 0 zerfällt in zwei Hälften.
  Die **statische** Hälfte — alle Fundstellen aus §1.1 und §1.2 sowie X4
  und X6 an Datei:Zeile gegengeprüft — ist erhoben und bestätigt alle
  Aussagen dieser Spec. Die **empirische** Hälfte (A0.1, A0.3, A0.4)
  konnte nicht gemessen werden, weil keine Linux-Umgebung zur Verfügung
  stand; sie entfällt damit als Vorbedingung fürs Bauen und wird in den
  manuellen Tests nachgeholt (M1–M4, um die Paketnamen-Prüfung erweitert).
- **2026-09-22 · `ANNAHME A-1`, Paketnamen ungemessen:** Die Paketnamen in
  A5/A6 stehen als markierte Annahme im Code
  (`// ANNAHME A-1: Paketname aus A0.3 nicht gemessen`), abgeleitet aus
  §1.1. **Sie sind vor dem Merge durch M1–M4 zu bestätigen.** Konkreter
  Zweifel: `gnome-keyring` allein genügt auf einer Minimal-Installation
  vermutlich nicht (`libpam-gnome-keyring`?), `kwalletd6` ohne
  `kwallet-pam` ebenso wenig. Fällt ein Messwert gegen die Spec aus, gilt
  §3 A0: der Messwert gewinnt, die Spec wird nachgezogen.

- **2026-09-22 · Review-Fund · Stefan:** „Entfernen" meldete Erfolg,
  obwohl das Secret im Schlüsselbund blieb — §6.3 X6 hatte die beiden
  Nutzer-Pfade fälschlich als Aufräumpfade geführt. Entschieden: ehrlich
  melden, aber nicht blockieren. Eingearbeitet als **A17** und als
  Korrektur in X6.

*(weitere Klarstellungen werden während der Umsetzung nachgetragen:
Datum · Frage-ID · Antwort)*
