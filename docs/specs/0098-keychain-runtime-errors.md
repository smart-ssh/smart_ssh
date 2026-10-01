# Spec 0098 — Schlüsselbund-Fehler zur Laufzeit ohne Bibliothekstext

Status: freigegeben · Backlog: BL-0244, BL-0205, BL-0206 · Gate: release-1.0/A
Zweck: Jeder Fehler des Schlüsselbunds, der nach dem Start auftritt, erreicht das Frontend als übersetzte Meldung mit stabilem Code und ohne den Text der `keyring`-Bibliothek, und eine einzelne Ablehnung ändert den Verfügbarkeitszustand nicht.
Review-Priorität: ERHÖHT

## 1. Ist-Stand (Stand `00f65a6`)

**Übersetzung nur bei „beim Start nicht verfügbar“.**
`keychain_aware_credential_error` (`crates/app-logic/src/error.rs`) ersetzt
`CredentialError::Backend` nur dann durch `KEYCHAIN_UNAVAILABLE`, wenn
`AppState.keychain` nicht `Available` ist. Sonst geht der Fehler über
`CommandError::from` mit `code: None` hinaus. Die Meldung ist dann
`CredentialError`s `Display`, also „Credential-Backend-Fehler: <Text der
Bibliothek>“ (gelesen, nicht ausgeführt). Das betrifft alle Aufrufer der
Funktion, darunter das Speichern von API-Keys, Server-Passwörtern,
Passphrasen und Sudo-Passwörtern (BL-0244), und das Lesen nach einer Sperre
des Schlüsselbunds (BL-0205).

**Der Zustand ändert sich nach dem Start nicht.** `AppState.keychain` wird
beim Start ermittelt (`credentials_keyring::probe_keychain_availability`,
aufgerufen in `app-shell/src/lib.rs`) und
höchstens dort eskaliert (`escalate_to_unavailable`, einziger Aufrufer beim
Start). Danach ändert ihn nichts mehr (Recherche: kein weiterer Aufrufer).

**Ausnahme `clear_sudo_password`** (`server_credentials.rs`): Dieser
Lösch-Weg läuft nicht über `keychain_aware_credential_error`, sondern gibt
bei jedem `Backend`-Fehler fest `KEYCHAIN_UNAVAILABLE` zurück, unabhängig vom
Startzustand (Spec 0071 A17, Kommentar „hier unbedingt“). Bestehende Tests
prüfen genau das.

**Ein bestehender Test sichert das alte Verhalten:**
`test_backend_error_keeps_its_message_when_the_keychain_is_available`
(`error.rs`) verlangt `code == None` und den Bibliothekstext in `message`.

**Die Bibliothek unterscheidet „gesperrt“ nicht verlässlich.** Gemessen am
Quelltext der eingesetzten Stores (`keyring` 4.1.6, `keyring-core` 1.0.0):
- macOS (`apple-native-keyring-store` 1.0.2, `keychain.rs`): Nur
  `-61`, `-25244`, `-25291`, `-25292`, `-25294`, `-25295` werden zu
  `NoStorageAccess`. Alles andere wird `PlatformFailure`, darunter
  abgebrochener Dialog (`-128`), Interaktion nicht erlaubt (`-25308`) und
  Authentifizierung fehlgeschlagen (`-25293`).
- Linux (`zbus-secret-service-keyring-store` 1.0.1): `NoStorageAccess` für
  Zugriffsfehler des Dienstes.
- Windows (`windows-native-keyring-store` 1.1.0): `NoStorageAccess` nur für
  `ERROR_NO_SUCH_LOGON_SESSION`.

Ein gesperrter Schlüsselbund und ein einzelnes „Nicht erlauben“ sehen auf
macOS also gleich aus. `credentials-keyring` gibt ohnehin nur
`CredentialError::Backend(e.to_string())` weiter, die Art geht verloren.

**Verbindungsaufbau und Verbindungstest.** `resolve_auth` in
`crates/core/src/ssh/auth.rs` macht aus einem Lesefehler
`SshError::CredentialResolutionFailed(format!("Passwort: {e}"))`, ebenso für
Private Key, Passphrase, Zertifikat und Key. `{e}` enthält den
Bibliothekstext. Der Code ist `SSH_CREDENTIAL_RESOLUTION_FAILED`.
- Verbindungstest (`test_connection.rs`): `NetworkError { message:
  other.to_string(), code: Some(other.code()) }`. Das Frontend
  (`ServerForm`, Fall `networkError`) zeigt bei bekanntem Code die
  Übersetzung „Zugangsdaten konnten nicht aufgelöst werden“. Der rohe Text
  steht aber weiter im Feld `message` des DTO (gelesen, nicht ausgeführt).
  BL-0206 beschreibt „✗ Netzwerkfehler: Passwort: No default store …“. Das
  ist der Stand vor dem Code. Heute fehlt noch der Hinweis auf den
  Schlüsselbund als Ursache.
- Verbindungsaufbau (`commands/connect.rs`): dieselbe `SshError`-Meldung als
  `message` (Kommentar Spec 0094, A1.7), Code ebenso.
- `name_hop` (`ssh-transport/src/auth.rs`) setzt vor jede
  `CredentialResolutionFailed`-Meldung `user@host:port: ` (Spec 0076 A-8).
- `resolve_auth` läuft je Hop erst nach TCP-Verbindung, Handshake und
  Host-Key-Prüfung (`ssh-transport/src/connect.rs`). Der `MockConnector` im
  Verbindungstest liest keine Credentials. Fehler ab dem zweiten Hop sind
  gegen den Testserver nicht zuverlässig erreichbar
  (`ssh-transport/tests/integration.rs`, ADR 0008).

**Stellen, die den Fehler verwerfen** (`.ok()`, `let _ =`, nur
`tracing::warn!`), etwa das Sudo-Passwort in `connect.rs` oder
`risk_second_opinion.rs`: Sie zeigen keinen Text an und sind Nicht-Ziel (§3).

**Frontend:** `KNOWN_ERROR_CODES` in `errorCodes.ts`, Texte unter
`errors.<CODE>` in `locales/de|en/common.json`. `translateErrorCode` ersetzt
den Text nur bei bekanntem Code.

## 2. Teil 0

Teil 0: entfällt. Der Lösungsweg hängt nicht davon ab, welche Fehlerart ein
gesperrter Schlüsselbund auf welcher Plattform liefert. A1 behandelt alle
Arten gleich (§1, Messung am Quelltext der Stores).

## 3. Ziel und Nicht-Ziele

Ziel: Kein Schlüsselbund-Fehler zur Laufzeit erreicht das Frontend mit dem
Text der Bibliothek, weder angezeigt noch im DTO. Der Nutzer sieht, dass der
Schlüsselbund die Ursache ist.

Nicht-Ziele:
- Kein erneutes Prüfen der Verfügbarkeit zur Laufzeit. Der Zustand aus dem
  Start bleibt die einzige Quelle für `KEYCHAIN_UNAVAILABLE` (Spec 0071 A16).
- Keine Unterscheidung nach Fehlerart (gesperrt / abgelehnt / Plattform) in
  der Meldung. Die Stores liefern das nicht verlässlich (§1).
- Stellen, die den Fehler heute verwerfen, bleiben, wie sie sind.
- `CredentialError::NotFound` bleibt fachlich „kein Eintrag“ (Spec 0071 A14,
  I4).
- Kein neuer Verfügbarkeitszustand und keine Änderung an der Diagnose-Ansicht.

## 4. Anforderungen

- **A1 MUSS:** Ein `CredentialError::Backend` bei verfügbarem Schlüsselbund
  erreicht das Frontend mit einem neuen stabilen Code `KEYCHAIN_ACCESS_FAILED`
  und einer festen Meldung ohne die Nutzlast des Fehlers. Das gilt für alle
  Wege, die heute `keychain_aware_credential_error` nutzen, lesend wie
  schreibend, **und für `clear_sudo_password`**. Dieser Weg wertet künftig
  den Startzustand aus wie die übrigen (A2). Das ändert Spec 0071 A17: Bei
  verfügbarem Schlüsselbund meldet er `KEYCHAIN_ACCESS_FAILED` statt
  `KEYCHAIN_UNAVAILABLE`.
- **A2 MUSS:** Bei nicht verfügbarem Schlüsselbund bleibt es bei
  `KEYCHAIN_UNAVAILABLE` (unverändert).
- **A3 MUSS:** Ein Fehler zur Laufzeit ändert `AppState.keychain` nicht. Nach
  einem einzelnen fehlgeschlagenen Zugriff gelingt der nächste Zugriff,
  sobald der Store wieder antwortet, und keine Meldung behauptet „nicht
  verfügbar“.
- **A4 MUSS:** Scheitert beim Verbindungsaufbau oder Verbindungstest das
  Lesen eines Secrets mit einem Backend-Fehler, auf jedem Hop einschließlich
  der Jump-Hosts, dann trägt das Ergebnis einen Code, der den Schlüsselbund
  als Ursache nennt. Bei verfügbarem Schlüsselbund ist das
  `KEYCHAIN_ACCESS_FAILED`, bei nicht verfügbarem `KEYCHAIN_UNAVAILABLE`.
  Es steht nicht unter „Netzwerkfehler“ und nicht als „Zugangsdaten konnten
  nicht aufgelöst werden“. Fehlt der Eintrag (`NotFound`), bleibt
  `SSH_CREDENTIAL_RESOLUTION_FAILED`.
- **A5 MUSS:** In den Fällen aus A1 und A4 enthält kein Feld, das ans
  Frontend geht (`message` eingeschlossen), den Text der Bibliothek. Erlaubt
  sind die Art des Secrets („Passwort“, „Passphrase“ …), feste Texte und die
  Hop-Angabe `user@host:port` aus Spec 0076 A-8.
- **A6 MUSS:** `KEYCHAIN_ACCESS_FAILED` ist in DE und EN übersetzt und steht
  in `KNOWN_ERROR_CODES`. Inhalt: Zugriff auf den Schlüsselbund ist
  fehlgeschlagen, möglicherweise ist er gesperrt oder der Zugriff wurde
  abgelehnt; erneut versuchen; genauer Grund im Log. Der Wortlaut ist Sache
  des Coders, aber ohne „nicht verfügbar“ und ohne Paket- oder
  Installationshinweise.
- **A7 SOLL:** Der Text der Bibliothek bleibt zur Diagnose erhalten, in einem
  Log-Eintrag auf `debug`. Für Log-Einträge gelten die bestehenden Regeln
  aus Spec 0094. Bestehende `warn`-Einträge mit dem `CredentialError`
  bleiben unverändert, etwa in `server_credentials.rs`. Spec 0094 nimmt
  Schlüsselbund-Fehler aus.

## 5. Design

- Wie der Backend-Fehler von `core` (`resolve_auth`) bis zum DTO als
  Schlüsselbund-Fehler erkennbar bleibt, entscheidet der Coder, zum Beispiel
  eine eigene `SshError`-Variante oder ein Code am bestehenden Fehler.
  Vorgabe: `core` bleibt ohne Abhängigkeit von `AppState`. Die Unterscheidung
  zwischen A1 und A2 fällt dort, wo `AppState.keychain` bekannt ist.
- Eine neue `SshError`-Variante oder ein neuer Code ändert keine öffentliche
  Schnittstelle. Die Form von `CommandError` und `TestConnectionResult`
  bleibt gleich, es kommt nur ein Code-Wert hinzu.

## 6. Sicherheits-Invarianten

- **Spec 0071 X2 (kein Secret über die Fehlerkette):** wird ausgeweitet. Die
  Nutzlast von `Backend` erreicht das Frontend auf keinem der Wege aus A1/A4
  mehr, auch nicht im Feld `message`.
- **Spec 0071 A14/I4 (Unbekannt ist nicht „nein“):** unberührt, `NotFound`
  bleibt getrennt.
- **Spec 0071 A16 (ein Zustand je Programmlauf):** unberührt (A3).
- **Spec 0071 A17:** geändert für den Fall „verfügbar“ (A1), siehe dort.
- **Spec 0094 (Log-Regeln):** A7 nutzt nur `debug`.
- **`CredentialStore`:** Das Verhalten beim Lesen und Schreiben ändert sich
  nicht, nur die Fehlerdarstellung.

## 7. Tests

Grundlage sind die fehlschlagenden Test-Stores in
`app-logic/src/test_support.rs`. Sie haben heute feste Nutzlast-Texte. Sie
werden erweitert um eine einstellbare Nutzlast und um einen Store, der nur
einmal scheitert, und um einen Store, dessen `get` nur für einen bestimmten
Eintrag scheitert (wie heute schon `set` für einen Slot). Die Nutzlast ist in
jedem Test ein Marker wie
`"LIBTEXT-0098 Geheim-0098"`.

Der bestehende Test
`test_backend_error_keeps_its_message_when_the_keychain_is_available`
sichert das Verhalten, das A1 ändert. Er wird durch T1 ersetzt, nicht
gelockert. Die bestehenden Tests zu `clear_sudo_password` werden auf A1/A2
umgestellt.

**Testebenen für den Verbindungsweg** (§1: Fehler ab dem zweiten Hop sind
gegen den Testserver nicht erreichbar):
- **Ebene V1:** ein Test-`Connector` im Verbindungstest, der für jeden Hop
  die echte Auflösung der Secrets (`resolve_auth`) mit dem Test-Store
  aufruft. Er prüft Code und `message` des `TestConnectionResult`.
- **Ebene V2:** ein Integrationstest am ersten Hop gegen den Testserver in
  `ssh-transport/tests`, mit eigenem fehlschlagendem Store dort, denn
  `ssh-transport` hängt nicht von `app-logic` ab. Er prüft, dass der Fehler die Transport-Schicht als
  Schlüsselbund-Fehler erkennbar und ohne Marker verlässt.

- **T1 (A1, Schreiben):** Server-Passwort speichern (app-logic), Store
  scheitert, Schlüsselbund verfügbar → Code `KEYCHAIN_ACCESS_FAILED`, Meldung
  ohne Marker. Für die Wege in `app-shell` (API-Key, an `tauri::State`
  gebunden) genügt ein Test auf Funktionsebene an der gemeinsamen
  Übersetzung. Scheitert am heutigen Stand (`code: None`, Marker in
  `message`).
- **T2 (A1, Lesen):** ein lesender Weg, z. B. das Lesen eines Secrets im
  Verbindungstest für den Ziel-Hop oder `identity_file` → wie T1.
- **T3 (A2):** dieselben Fälle mit `KeychainAvailability::Unavailable` →
  `KEYCHAIN_UNAVAILABLE`, unverändert.
- **T3a (A1/A2, `clear_sudo_password`):** Löschen scheitert. Bei
  verfügbarem Schlüsselbund kommt `KEYCHAIN_ACCESS_FAILED` ohne Marker, bei
  nicht verfügbarem `KEYCHAIN_UNAVAILABLE`. Scheitert am heutigen Stand im
  ersten Fall.
- **T4 (A3):** Erst scheitert der Store, dann gelingt er (Test-Store mit
  einmaligem Fehler). Der erste Fehler trägt nicht `KEYCHAIN_UNAVAILABLE`,
  der zweite Aufruf gelingt, `AppState.keychain` ist danach `Available`.
  Scheitert, wenn ein Laufzeitfehler „nicht verfügbar“ meldet oder den
  Zustand eskaliert.
- **T5 (A4, Jump-Host, Ebene V1):** Verbindungstest mit einem Jump-Host,
  dessen Passwort-Lesen mit Backend-Fehler scheitert. Bei verfügbarem
  Schlüsselbund kommt `KEYCHAIN_ACCESS_FAILED`, bei nicht verfügbarem
  `KEYCHAIN_UNAVAILABLE` (Akzeptanz BL-0206). In beiden Fällen ist
  `message` ohne Marker. Scheitert am heutigen Stand
  (`SSH_CREDENTIAL_RESOLUTION_FAILED`, Marker in `message`).
- **T6 (A4, Ziel-Hop und Aufbau):** Ebene V1 für den Ziel-Hop — **für diesen
  nicht herstellbar, s. §9.3**. Ebene V2 für
  den ersten Hop über `ssh_transport::connect`. Die Abbildung auf den Code
  beim Verbindungsaufbau (`connect.rs`) wird auf Funktionsebene geprüft. Der
  Bericht nennt, was davon ohne echte Verbindung nicht abgedeckt ist.
- **T7 (A4, NotFound):** fehlender Eintrag auf einem Jump-Host →
  `SSH_CREDENTIAL_RESOLUTION_FAILED`, nicht `KEYCHAIN_ACCESS_FAILED`.
- **T8 (A6):** Frontend: `KEYCHAIN_ACCESS_FAILED` ist bekannt und in DE/EN
  übersetzt. `translateErrorCode` liefert den übersetzten Text, nicht den
  Fallback. Dazu kommt ein Fall in den Tests der Ergebnisanzeige des
  Verbindungstests (`TestResultBadge`): Ergebnis `networkError` mit diesem
  Code zeigt weder „Netzwerkfehler“ noch den Marker.

Adversarial (ERHÖHT):
- **T9:** Nutzlast mit Zeilenumbruch und Anführungszeichen
  (`"x\nPasswort: Geheim-0098\""`) → in keinem Feld ans Frontend.
- **T10:** Nutzlast, die selbst wie ein Code aussieht (`"KEYCHAIN_UNAVAILABLE"`)
  → Code bleibt `KEYCHAIN_ACCESS_FAILED`, die Nutzlast fließt nicht in die
  Code-Wahl ein.
- **T11:** Fehler bei `delete` auf einem Weg, der ihn heute anzeigt → wie T1.
  Wege, die ihn verwerfen, bleiben unverändert (Nicht-Ziel).
- **T12 (Ebene V1):** Jump-Host-Kette mit drei Hops, Fehler am mittleren →
  Code nach A4, kein Marker.
- **T12a (Ebene V2):** Am ersten Hop bleibt die Hop-Angabe nach Spec 0076
  A-8 (`user@127.0.0.1:<port>`) in der Meldung, auch wenn der Fehler künftig
  als Schlüsselbund-Fehler erkennbar ist. `name_hop` läuft für jeden Hop
  gleich, deshalb genügt der erste Hop.
- **T13:** Passphrase eines Private Keys auf dem Jump-Host scheitert → A4 gilt
  für alle Secret-Arten aus `resolve_auth`, nicht nur für „Passwort“.

## 8. Offene Punkte

Keine.

## 9. Klarstellungen

Während der Umsetzung nachgetragen. Keine davon ändert eine Anforderung;
alle drei korrigieren eine Annahme über den Ist-Stand bzw. die Testebene.

1. **Zu §1, „Ein bestehender Test sichert das alte Verhalten":** Es sind
   **zwei**. Neben `test_backend_error_keeps_its_message_when_the_keychain_
   is_available` (`error.rs`) verlangte auch
   `test_failed_write_with_an_available_keychain_keeps_its_ordinary_error`
   (`server_credentials.rs`) `code == None`. Beide sind nach §7 durch
   T1-Tests ersetzt, nicht gelockert.
2. **Zu §7 T2:** Der lesende Weg liegt im Verbindungstest, nicht in
   `resolve_auth_method`. Die Funktion **liest nie** — ein leeres
   Formularfeld behält dort nur den bestehenden `CredentialRef`. §7 nennt den
   Verbindungstest als Alternative, diese gilt.
3. **Zu §7 T6, „Ebene V1 für den Ziel-Hop":** Für den Ziel-Hop ist diese
   Ebene strukturell nicht herstellbar, weil er sein Secret **vor** der Kette
   auflöst:
   - Ist das Formularfeld gefüllt, liegt das Secret im
     `EphemeralCredentialStore`; der scheitert nie, ein Schlüsselbund-Fehler
     kann dort nicht entstehen.
   - Ist es leer, liest `resolve_final_hop_auth` das gespeicherte Secret vor
     dem Verbindungsversuch. Der Fehler kommt dann als `CommandError` — der
     Weg, den T2 prüft —, nicht als `TestConnectionResult`.

   Ebene V1 bleibt damit für die **Jump-Hosts** zuständig (T5, T12, T13).
   `test_spec_0098_t6_the_target_hop_secret_is_resolved_before_the_chain`
   hält beide Hälften des Befunds fest.

## Umsetzung

**Teil 0:** entfällt.

**Reihenfolge:**
1. `fix(app-logic): report keychain failures at runtime with a stable code instead of the library text [BL-0244]` — A1–A3, A5, A7 für die Wege über `keychain_aware_credential_error` und `clear_sudo_password`, T1–T4 (mit T3a), T9–T11.
2. `fix(core): keep credential-store failures recognisable through connection setup and the connection test [BL-0206]` — A4, A5, A7, T5–T7, T12, T12a, T13.
3. `feat(frontend): translate the keychain access failure [BL-0205]` — A6, T8.
4. `docs(changelog): note the readable keychain error messages [BL-0244]` — Fragment unter `changelog.d/`.

**Priorität:** ERHÖHT. Angriffsrichtungen für den Review:
- Die Nutzlast landet doch in einem Feld, etwa über `to_string()` eines
  äußeren Fehlers, `format!("{e}")` in einer Zwischenschicht oder `Debug`.
- Die Code-Wahl hängt am Text der Nutzlast statt an Variante und Zustand
  (T10).
- Ein Laufzeitfehler eskaliert den Zustand oder schreibt ihn um (T4).
- `NotFound` wird als Schlüsselbund-Fehler gemeldet und verletzt I4 (T7).
- Ein Weg über `ssh_transport::connect` oder über die Jump-Hop-Auflösung wird
  vergessen (T5, T6, T12).
- Ein **neuer** Log-Eintrag aus A7 landet über `debug` hinaus. Bestehende
  `warn`-Einträge sind ausgenommen (A7).

**Aufteilung:** ein Lauf auf Opus. Credential-Handling und Fehlerkette sind
der Kern, der Frontend-Teil ist klein (ein Code, zwei Texte).

**Berührte Module:** `crates/app-logic` (`error.rs`, `server_credentials.rs`
mit neuer Signatur von `clear_sudo_password`, `test_connection.rs`, Tests),
`crates/app-shell/src/commands/servers.rs` (Aufrufer), `crates/core/src/ssh` (`auth.rs`, `error.rs`), `crates/ssh-transport`
(Fehlerabbildung), `crates/app-shell/src/commands/connect.rs`,
Frontend `errorCodes.ts` und Locales, `changelog.d/`.

**Melde zurück:** für T1 und T5 den Beleg, dass sie am alten Stand scheitern;
die Liste aller angepassten Wege; wie der Fehler die Kette von `core` bis zum
DTO übersteht; manuelle Prüfung für macOS (Schlüsselbund sperren bzw. Dialog
ablehnen) als Testablauf.
