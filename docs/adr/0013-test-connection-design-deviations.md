# 0013-test-connection-design-deviations

## Status
Accepted

## Kontext

`docs/specs/0008-server-management-ui.md`, Abschnitt 7 skizziert
`test_connection` nur grob: "eigener Pfad, der Verbindungsdaten direkt aus
`ServerInput` nimmt statt aus dem `CredentialStore`", "nutzt denselben
`HostKeyStore` wie der reguläre `connect()`-Pfad, sodass eine während des
Tests bestätigte Host-Key-Vertrauensstellung auch beim späteren echten
Verbindungsaufbau greift". Beim Implementieren ergaben sich vier Stellen,
an denen diese Skizze entweder unvollständig war oder mit der bestehenden
Architektur aus Spec 0007 kollidierte.

**1. Leere Secret-Felder brauchen einen Bezugspunkt.** Spec 0008 Abschnitt
4 definiert für `update_server`: ein leeres Secret-Feld in `ServerInput`
heißt "unverändert lassen, bestehendes Credential weiterverwenden". Genau
dieses Verhalten verlangt Abschnitt 7 auch für `test_connection` — aber
`test_connection(input: ServerInput)` allein kann nicht wissen, **welcher**
gespeicherte Server gemeint ist, wenn beispielsweise die Passphrase leer
gelassen wird. `ServerInput` selbst enthält keine Server-ID (folgerichtig,
`create_server` braucht sie ja nicht).

**2. `PendingHostKeyConfirmation` aus dem regulären `connect()`-Pfad
(Spec 0007) trägt nur einen Fingerprint, keinen `raw_key`.** Der reguläre
Trust-Flow ruft `host_key_store.trust()` aber mit dem rohen Public Key auf,
nicht mit dem Fingerprint — `HostKeyStore::trust(host, port, key: &[u8])`.
Ohne den rohen Key in `TestConnectionResult::HostKeyUnknown`/`Mismatch`
könnte das Frontend nach einer Nutzerbestätigung im Test-Pfad gar nicht
`trust()` aufrufen.

**3. `ssh_transport::connect()` ist eine freie Funktion, kein Trait-Objekt.**
`SshTransport` (das Ergebnis eines *erfolgreichen* Verbindungsaufbaus) ist
bereits ein Trait und in bestehenden Tests mockbar. Der Verbindungsversuch
selbst — also genau das, was `test_connection` prüfen soll — hat aber keine
solche Abstraktion, weil bislang nichts ihn isoliert testen musste.

**4. `test_connection` erzeugt zwangsläufig frische Secrets (aus
`ServerInput` oder aus dem Fallback auf gespeicherte Credentials), die
laut Spec "nichts wird persistiert" nirgendwo landen dürfen** — aber die
bestehende `connect()`-Maschinerie (Spec 0007) erwartet einen echten
`&dyn CredentialStore`, aus dem sie anhand von `CredentialRef`s liest.

## Entscheidung

**`existing_server_id: Option<ServerId>`** als zusätzlicher Parameter von
`test_connection` (Command-Signatur und der zugrunde liegenden
Orchestrierungsfunktion in `crates/app-tauri/src/test_connection.rs`).
Beim Bearbeiten eines bestehenden Servers übergibt das Frontend dessen ID
mit; bei `create_server`-Formularen (noch kein Server vorhanden) bleibt er
`None`, und ein leeres Secret-Feld ohne `existing_server_id` ist dann ein
harter Fehler statt eines stillen Fallbacks auf nichts.

**`TestConnectionResult::HostKeyUnknown`/`HostKeyMismatch`** um
`raw_key: Vec<u8>` erweitert (zusätzlich zum/den Fingerprint(s) aus der
Spec-Skizze). Der neue **`trust_host_key(host, port, raw_key)`-Command**
ruft direkt `state.host_key_store.trust(...)` auf — denselben
`HostKeyStore`, den auch der reguläre `connect()`-Pfad nutzt (Spec 0007),
wodurch eine im Test bestätigte Vertrauensstellung beim späteren echten
Verbindungsaufbau tatsächlich greift, wie von der Spec gefordert.

**`Connector`-Trait** (`crates/app-tauri/src/test_connection.rs`) als
dünne Hülle um `ssh_transport::connect()`, mit `RealConnector` als einziger
Produktiv-Implementierung und `MockConnector` in Tests. Macht den
eigentlichen Verbindungsversuch — inklusive aller
`TestConnectionResult`-Varianten (Erfolg, Auth-Fehler, beide
Host-Key-Fälle, Netzwerkfehler, Timeout) — ohne echtes Netzwerk testbar.

**`EphemeralCredentialStore`** (`crates/app-tauri/src/ephemeral_credentials.rs`):
eine rein In-Memory-`CredentialStore`-Implementierung, die nur für die
Dauer eines einzelnen `test_connection`-Aufrufs existiert. Secrets aus
`ServerInput` (oder aus dem Fallback auf den echten `CredentialStore` bei
leerem Feld + `existing_server_id`) werden hier unter einer
Test-lokalen `CredentialRef` (`"test:password"` etc.) abgelegt und der
bestehenden `connect()`-Maschinerie unverändert als `&dyn CredentialStore`
übergeben. Der reale `CredentialStore` wird dabei ausschließlich lesend
angefasst (für den Fallback-Fall), nie schreibend — nichts aus einem Test
landet im Keychain/in der Datenbank.

## Konsequenzen

**Positiv:**
- Das in Abschnitt 7 geforderte "leeres Secret-Feld → gespeichertes
  Credential verwenden" ist für `test_connection` überhaupt entscheidbar.
- Eine im Testpfad bestätigte Host-Key-Vertrauensstellung ist beim
  regulären `connect()` direkt wirksam, ohne zweiten Trust-Mechanismus.
- Alle sechs `TestConnectionResult`-Varianten sind einzeln, deterministisch
  und ohne echtes Netzwerk testbar (Aufgabenstellung Teil 1, Punkt 6).
- Striktes "nichts wird persistiert": der reale `CredentialStore` wird im
  Testpfad nie beschrieben.

**Negativ / Trade-off:**
- `existing_server_id` und das erweiterte `HostKeyUnknown`/`HostKeyMismatch`
  (mit `raw_key`) sowie der `trust_host_key`-Command weichen von der
  Spec-Skizze aus Abschnitt 7 ab — wer nur die Spec liest, kennt keinen der
  drei.
- Der `Connector`-Trait ist eine zusätzliche Abstraktionsschicht, die es
  nur wegen der Testbarkeits-Anforderung gibt; ohne sie wäre der Code an
  dieser einen Stelle einfacher, aber der Verbindungsversuch selbst nicht
  isoliert testbar.
- Zwei parallele `CredentialStore`-Implementierungen im Testpfad (die
  echte, nur lesend, plus die `EphemeralCredentialStore`) statt eines
  einzigen Zugriffswegs — Preis für die "nichts wird persistiert"-Vorgabe.
