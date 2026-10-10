# Spec 0101 — Ganze Datenbank verschlüsseln, Secrets in die Datenbank, Master-Passwort

Status: umgesetzt
Zweck: Die Datenbankdatei ist vollständig verschlüsselt (SQLCipher), alle Secrets liegen darin, und ihr Schlüssel kommt entweder aus dem OS-Schlüsselbund oder aus einem Master-Passwort.
Bezüge: Spec 0036 (Chat-Inhalte), Spec 0059 (Startfehler), Spec 0071 und 0098 (Schlüsselbund), Spec 0094 (Logregeln), Spec 0096 (Rohdatei-Nachweis), ADR 0093, ADR 0106 (Sperre des Datenverzeichnisses), ADR 0121 (zweiter Start).
Review-Priorität: ERHÖHT (Verschlüsselung, Credentials, Migration, Start)

## Entscheidungen

- **E1** Der Datenbankschlüssel wird aus einem vorhandenen Zufallsschlüssel
  abgeleitet, dem **Wurzelschlüssel K**; es gibt keinen weiteren Eintrag.
- **E2** Alle Secrets (Server-Passwörter, Private Keys, Passphrasen,
  Zertifikate, Sudo-Passwörter, API-Keys, MCP-Token) liegen in der Datenbank;
  im Schlüsselbund bleibt höchstens K.
- **E3** Ist der Schlüsselbund beim Start nicht erreichbar, bietet die
  Anwendung „Erneut versuchen" / „Beenden"; nichts wird angefasst.
- **E4** Ist die Datei verschlüsselt und K fehlt, wird nie still ein neuer K
  erzeugt; „Neu anfangen" ist nur eine bewusste Wahl, die alte Datei wird nur
  umbenannt.
- **E5** Eine Klartext-Datenbank wird beim ersten Start automatisch
  umgewandelt, das Original nach erfolgreicher Prüfung entfernt.
- **E6** Ältere Versionen können die Datei nicht mehr öffnen (Changelog).
- **E7** Alte Schlüsselbund-Einträge werden gelöscht, sobald jedes Secret aus
  der Datenbank zurückgelesen und gleich ist.
- **E8** Ein Master-Passwort ist für alle wählbar, Standard ist der
  Schlüsselbund, der Wechsel geht in beide Richtungen; ohne Schlüsselbund
  bietet der Startdialog es an.
- **E9** Im Passwort-Modus liegt K, mit Argon2id(Passwort) verpackt, in einer
  Datei neben der Datenbank; Wechsel und Passwortänderung verpacken nur neu.
- **E10** Es gibt keine Wiederherstellung bei vergessenem Passwort; beim
  Einrichten gibt es Warnung und Bestätigung.
- **E11** Die frühere feldweise Verschlüsselung von Chat, Ledger, Historie und
  Zusammenfassungen ist zurückgenommen (Issue #113): diese Inhalte liegen als
  Klartext in der verschlüsselten Datei, vorhandene Einträge werden beim ersten
  Start einmal umgestellt (Spec 0036, Abschnitt 3).

## 1. Grundlagen und Messungen

**Datenbank.** Eine Datei `smart-ssh.db` im Datenverzeichnis (Spec 0004). Alle
Stores teilen eine Verbindung. Kein anderer Prozess öffnet die Datei.

**Schlüssel und Secrets vor dieser Spec.** K liegt im Schlüsselbund; Secrets
liegen dort je Server und Slot (`password`, `private_key`, `passphrase`,
`certificate`, `certificate_key`, `sudo_password`) und je KI-Anbieter. Die
Verweise darauf stehen in der Datenbank (Anbieter-Konfiguration,
Anmeldeart der Server). Bei fehlendem K wurde früher still ein neuer erzeugt.

**Messungen** (macOS; Linux wo genannt):

- SQLCipher 4.10 mit mitgebautem OpenSSL neben dem SQL-Treiber: alle
  Migrationen laufen, WAL ist mitverschlüsselt, alle Tests grün, `cargo audit`
  ohne Funde (macOS, Linux). Die CI baut und testet mit SQLCipher auf Ubuntu,
  Windows und macOS grün. Ungemessen bleiben die Release-Ziele
  `universal-apple-darwin` und `ubuntu-22.04`.
- Ein roher Schlüssel in der Form `"x'<64 Hex>'"` (mit den doppelten
  Anführungszeichen) funktioniert; ohne sie scheitert SQLite mit `code 1`.
- Falscher oder fehlender Schlüssel: Die Verbindung öffnet, die erste
  Abfrage liefert `code 26 „file is not a database"`; läuft zuerst eine
  Migration, kommt `code 7 „out of memory"`. Falscher Schlüssel und
  beschädigte Datei sind so nicht unterscheidbar.
- `cipher_log_level` steht standardmäßig auf `WARN`; unter Linux schreibt
  das auf stderr, deshalb `NONE`.
- Der Export in eine verschlüsselte Kopie überträgt weder `journal_mode`
  (danach `delete`) noch `user_version` (danach 0); auf der geöffneten Kopie
  gesetzt, bleiben beide erhalten.
- SQLite sinkt durch SQLCipher von 3.51.3 auf 3.50.4.

## 2. Vorab geklärte Fragen

- **Frage 1 — Baut die Abhängigkeit auf Windows?** Ja, siehe Abschnitt 1.
- **Frage 2 — Synchroner Secret-Speicher auf asynchroner Verbindung.** Der
  synchrone Credential-Speicher überbrückt zur asynchronen Datenbank über
  einen blockierenden Arbeitsthread. Das setzt eine Multi-Thread-Runtime
  voraus (auf einer Single-Thread-Runtime panickt es, gemessen); ein Wechsel
  der Runtime-Konfiguration muss in einem Test auffallen. Beim Aufbau des
  Anwendungszustands läuft noch keine Runtime; der Griff darauf wird dort
  ausdrücklich beschafft.
- **Frage 3 — Startablauf im Passwort-Modus.** Im Passwort-Modus entsteht das
  Fenster, bevor der Zustand da ist; der Startablauf läuft deshalb aus einem
  Kommando heraus. Native Dialoge tragen das auf macOS nicht, und sie haben
  keine Texteingabe. Entsperr- und Einrichtemasken sowie die Startdialoge
  D1–D4 erscheinen daher **im Fenster**; die Entscheidungslogik bleibt
  dieselbe. Vor der Entsperrung ist ein Kommando mit nicht verwaltetem
  Zustand ein sichtbarer Fehler, kein Panic.

## 3. Ziel und Nicht-Ziele

Ziel: Keine Datei im Daten- und im Konfigurationsverzeichnis enthält
Hostnamen, Benutzernamen, Header-Werte, Secrets oder das MCP-Token im
Klartext. Bestehende Installationen wandeln ohne Datenverlust um. Ohne
Schlüssel startet nichts halb, und nichts wird still neu erzeugt.

Nicht-Ziele:

- Sicheres Überschreiben des alten Klartexts auf dem Datenträger, in Backups
  oder Schnappschüssen (Changelog).
- Eine verständliche Meldung in älteren Versionen: entfällt wegen E6; Ersatz
  ist der Changelog-Hinweis (A22).
- Maskierte Anzeige der Provider-Header in der Oberfläche.
- Export/Import, Sperren nach Leerlauf, Biometrie.

## 4. Anforderungen

### Etappe 1 — SQLCipher, Schlüssel, Umwandlung

- **A1** SQLite wird mit SQLCipher gebaut. `cargo deny` und `cargo audit`
  bleiben grün; die erzeugten Drittlizenzen enthalten die Lizenztexte von
  SQLCipher und OpenSSL.
- **A2** Datenbankschlüssel = HKDF-SHA256(K, `info` = `smart-ssh/db-key/v1`),
  32 Byte, als roher Schlüssel `x'…'` (kein PBKDF2). Weder K noch der
  abgeleitete Schlüssel erscheinen in Log, Fehlertext, DTO oder
  Diagnosepaket.
- **A3** Der Start entscheidet nach der Tabelle unten, **bevor** eine
  Migration läuft. Der Dateizustand wird am Header erkannt, ohne die Datei zu
  ändern: *fehlt* (keine Datei, oder 0 Byte ohne nichtleere `-wal`),
  *Klartext* (`SQLite format 3\0`), *sonst*. K-Zustand: *da*, *NotFound*,
  *nicht erreichbar*, *ungültig* (Inhalt unbrauchbar; Verpackungsdatei,
  deren Format oder Version nicht lesbar ist). Maßgeblich ist das Ergebnis
  des Lesens von K, nicht die Probe beim Start. Im Passwort-Modus ist K erst
  nach der Entsperrung (A16) *da*; falsches Passwort ist kein Tabellenfall.

  | Datei \ K | da | NotFound | nicht erreichbar | ungültig |
  |---|---|---|---|---|
  | fehlt | neu anlegen | K erzeugen, neu anlegen | Dialog D1 | Dialog D3 |
  | Klartext | umwandeln (A6) | K erzeugen, umwandeln | Dialog D1 | Dialog D4 |
  | sonst | öffnen; nicht lesbar → Dialog D2 | Dialog D2 | Dialog D1 ohne Einrichten | Dialog D3 |

  - **D1** „Erneut versuchen" / „Beenden"; zusätzlich „Master-Passwort
    einrichten" — **nur** bei Datei *fehlt* oder *Klartext* **und** Grund
    `NoSecretServiceProvider` oder `NoSessionBus` (dort kann kein erreichbarer
    K existieren; bei `Locked`, `Unknown` oder einem Backend-Fehler würde ein
    neuer K den feldweise verschlüsselten Verlauf unlesbar machen). Bei
    *Klartext* nennt der Text den Verlust des bisherigen Verlaufs wie D4.
    „Erneut versuchen" prüft ohne Neustart erneut.
  - **D2** Eigener Code (`DB_KEY_MISMATCH`): Datei mit dem vorhandenen
    Schlüssel nicht lesbar bzw. kein Schlüssel zur verschlüsselten Datei.
    Text: beschädigt oder zu einem anderen Schlüssel gehörig, **kein**
    Hinweis auf „Backup einspielen" als erste Wahl. Knöpfe „Beenden" /
    „Neu anfangen" (A5).
  - **D3** „Schlüssel unbrauchbar": „Beenden" / „Neu anfangen" (A5). Der
    vorhandene Eintrag bzw. die Datei wird erst nach dieser Wahl ersetzt.
  - **D4** Wie D3, aber statt „Neu anfangen" „Neuen Schlüssel erzeugen": Die
    Klartext-Datei bleibt lesbar und wird mit neuem K umgewandelt (A6); nur
    der feldweise verschlüsselte Verlauf geht verloren, das sagt der Text.
    Die nicht mehr lesbaren Einträge entfernt die anschließende Umstellung
    (Spec 0036, U4) mit einem einmaligen Hinweis (U5). Zweite Bestätigung wie
    A5; im Passwort-Modus Reihenfolge wie A5 (neues Passwort zuerst, alte
    Verpackungsdatei umbenennen, nie überschreiben).

  In keinem Dialog-Fall wird die Datenbank geöffnet, verändert oder
  umbenannt, solange der Nutzer nicht gewählt hat. **Ein neuer K entsteht
  nur** in den beiden Feldern „K erzeugen" und nach einer ausdrücklichen Wahl
  „Neu anfangen" (D2, D3, A16), „Neuen Schlüssel erzeugen" (D4) oder
  „Master-Passwort einrichten" (D1).
- **A4** Nie „out of memory" oder ein Migrationsfehler für einen
  Schlüssel-Fall: Die Lesbarkeit wird vor der Migration geprüft.
- **A5** „Neu anfangen" verlangt eine zweite Bestätigung, benennt Datei,
  `-wal`, `-shm` und (Passwort-Modus) die Verpackungsdatei um in
  `<name>.unreadable-<UTC-Zeitstempel>`, löscht nichts, startet frisch und
  nennt im Dialog den neuen Dateinamen. Im Passwort-Modus bleibt der Modus:
  das neue Master-Passwort wird **zuerst** eingerichtet (A13), die neue
  Verpackungsdatei geschrieben, erst dann werden die alten Dateien
  umbenannt. Bricht der Nutzer das Einrichten ab, bleibt alles unverändert.
- **A6** Umwandlung einer Klartext-Datei, vor den Migrationen:
  1. Original öffnen, WAL vollständig einspielen, schließen;
  2. verschlüsselte Kopie in eine Zwischendatei schreiben; die Zwischendatei
     mit dem Schlüssel öffnen, `user_version` übernehmen und
     `journal_mode=WAL` setzen, schließen;
  3. Zwischendatei neu öffnen und prüfen: `integrity_check` = `ok`, je
     Tabelle gleiche Zeilenzahl, Migrationstabelle gleich, `user_version`
     gleich, `journal_mode` = `wal`; schließen;
  4. alte `-wal`/`-shm` entfernen, dann die Zwischendatei atomar an die
     Stelle des Originals umbenennen;
  5. Migrationen wie gewohnt.

  Scheitert ein Schritt vor 4: Original inhaltsgleich, Zwischendatei
  entfernt, Startfehler mit Code (kein Weiterlauf im Klartext). Eine
  liegengebliebene Zwischendatei wird beim nächsten Start verworfen, nie
  übernommen. Ist `smart-ssh.db` ein Symlink, wird nicht umgewandelt:
  Startfehler mit Meldung, nichts verändert.
- **A7** Die Umwandlung gelingt auch an einer Datei, die der Build vor dieser
  Spec (SQLite 3.51.3) mit allen damaligen 14 Migrationen geschrieben hat.
- **A8** `cipher_log_level` ist `NONE`. Keine Umleitung von stderr ins Log.

### Etappe 2 — Secrets in der Datenbank

- **A9** Ein produktiver Credential-Speicher hält Secrets in einer Tabelle der
  verschlüsselten Datenbank; der Anwendungszustand nutzt ihn. Semantik wie
  bisher: `NotFound` bleibt `NotFound`, `delete` ist idempotent, ein
  Backend-Fehler enthält kein Secret. Der Schlüsselbund dient danach nur noch
  K.
- **A9.1** Fehler dieses Speichers erreichen die Oberfläche mit einem eigenen
  Code (`SECRET_STORE_FAILED`) und ohne „Schlüsselbund" im Text, nicht mit
  `KEYCHAIN_*`, und hängen nicht vom Verfügbarkeitszustand des Schlüsselbunds
  ab — auch nicht beim Verbindungsaufbau und im Verbindungstest auf jedem
  Hop. Für das Lesen und Schreiben von K gilt Spec 0098 unverändert.
- **A10** Umzug beim ersten Start mit dem neuen Speicher: Verweise aus der
  Datenbank (Anbieter, Anmeldeart, plus die festen Sudo-Slots), je Verweis
  das Secret aus dem Schlüsselbund lesen, schreiben, zurücklesen,
  vergleichen; `NotFound` → ausgelassen. Zustand in der Datenbank: *offen* →
  *umgezogen, Löschen ausstehend* (erst wenn **alle** Verweise gleich sind) →
  *erledigt* (alle Einträge gelöscht oder `NotFound`). Beim Übergang nach
  *umgezogen* wird die Liste der zu löschenden Verweise festgehalten;
  gelöscht wird nach dieser Liste, nicht nach dem späteren Datenbankstand.
- **A11** Lesefehler (Backend) beim Umzug: Dialog D1 ohne Einrichten (im
  Passwort-Modus plus A11.1), nichts gelöscht, Zustand bleibt *offen*.
  Löschfehler im Zustand *umgezogen*: Warnung ins Log ohne Secret, die
  Anwendung startet, das Löschen wird bei jedem Start erneut versucht.
- **A11.1** Zustand *offen*, K aus dem Master-Passwort, Lesen beim Umzug
  scheitert (Backend-Fehler oder nicht verfügbar): Der Dialog bietet
  zusätzlich „Ohne Übernahme fortfahren" (Hinweis: gespeicherte Passwörter
  usw. neu eingeben). Diese Wahl setzt den Zustand *übersprungen*. In
  *übersprungen* wird **nie** ein Schlüsselbund-Eintrag gelöscht, auch wenn
  der Schlüsselbund später erreichbar ist.
- **A12** Das MCP-Token liegt in der Datenbank. Ein Token aus der
  Einstellungsdatei wird mit gleichem Wert übernommen, zurückgelesen, dann
  dort entfernt. Erzeugen und Erneuern schreiben nur in die Datenbank.

### Etappe 3 — Master-Passwort

- **A13** Einrichten — aus den Einstellungen, aus D1 (nur Datei
  *fehlt*/*Klartext*) oder im Passwort-Modus aus A5/D4: Passwort zweimal,
  mindestens 12 Zeichen, Warnung „ohne Passwort sind alle Daten verloren,
  keine Wiederherstellung" mit ausdrücklicher Bestätigung (E10). Reihenfolge:
  K (vorhanden oder neu nach A3) verpacken, Verpackungsdatei atomar
  schreiben, entpacken und mit K vergleichen, dann erst K aus dem
  Schlüsselbund löschen. Aus D1 gibt es keinen K im Schlüsselbund; dort wird K
  neu erzeugt (A3).
- **A14** Verpackungsdatei: versioniert; KDF `argon2id` mit Parametern, Salt
  ≥ 16 Byte zufällig je Verpacken, Nonce, Chiffrat von K mit
  ChaCha20-Poly1305 unter Argon2id(Passwort, Salt), Kopf als AAD. Parameter
  beim Schreiben m = 64 MiB, t = 3, p = 1; beim Entpacken werden Parameter
  darunter abgelehnt. Unix-Rechte 0600.
- **A15** Wechsel Passwort → Schlüsselbund: aktuelles Passwort, K in den
  Schlüsselbund schreiben, zurücklesen, vergleichen, dann die
  Verpackungsdatei entfernen. Passwort ändern: altes Passwort, neu verpacken
  (neues Salt), atomar ersetzen.
- **A16** Start im Passwort-Modus: Entsperrmaske. Bis zur Entsperrung ist
  kein Kommando außer Entsperren / Beenden / „Neu anfangen" erreichbar, der
  MCP-Server läuft nicht, keine Verbindung wird aufgebaut. Falsches Passwort →
  sichtbare Meldung, erneute Eingabe, kein Datenbankzugriff, nie ein neuer
  Schlüssel.
- **A17** Verpackungsdatei **und** Schlüsselbund-Eintrag (abgebrochener
  Wechsel): Die Verpackungsdatei gilt. Nach Entsperrung wird der Eintrag
  gelöscht, wenn er gleich K ist; sonst nichts gelöscht, Warnung ins Log. Ist
  der Schlüsselbund nicht erreichbar: nichts tun, Warnung ins Log. Scheitert
  die Authentifizierung der Verpackung, lautet die Meldung „Passwort falsch
  oder Datei beschädigt" — beides ist nicht unterscheidbar.
- **A18** Die Einstellungen zeigen den aktiven Modus.
- **A19** Passwort, K und abgeleitete Schlüssel liegen in Typen, die beim
  Freigeben überschrieben werden, und verlassen das Backend nicht.

### Etappe 4 — Texte

- **A20** Alle neuen Dialoge, Masken und Codes in DE und EN. Bestehende
  Texte, die Secrets im Schlüsselbund verorten, sagen „in der
  verschlüsselten Datenbank".
- **A21** Erststart-Hinweis nach Klarstellung 1.
- **A22** Changelog-Fragment: Datei verschlüsselt; ältere Versionen melden
  danach „möglicherweise beschädigt … Backup einspielen" — **das stimmt
  dann nicht, kein Backup einspielen**, sondern die neue Version nutzen;
  Secrets ziehen in die Datenbank; alter Klartext kann in Backups liegen;
  Master-Passwort ohne Wiederherstellung. README: Linux braucht Secret
  Service **oder** ein Master-Passwort.

### Instanzen

- **A23** Vor jedem Zugriff auf Datenbank und Verpackungsdatei sperrt die
  Anwendung das Datenverzeichnis exklusiv, bis zum Prozessende. Hält ein
  anderer Prozess die Sperre, endet der Start mit dem Fehler „läuft bereits",
  ohne die Datenbank anzufassen (ADR 0106). Die Umwandlung (A6) setzt die
  Sperre voraus.
- **A24** Ein zweiter Start desselben Release-Builds mit dem
  **Standard**-Datenverzeichnis holt das offene Fenster der laufenden Instanz
  nach vorn und endet, ohne die Datenbank anzufassen. Ist
  `SMART_SSH_DATA_DIR` gesetzt (nicht leer), holt ein Start keine andere
  Instanz nach vorn: Mit einem anderen Datenverzeichnis startet er normal und
  läuft daneben, mit demselben endet er an der Sperre (A23; ADR 0121).

## 5. Startablauf

1. Log. 2. Dateizustand (A3). 3. K beschaffen: Verpackungsdatei vorhanden →
Entsperren (A16), sonst Schlüsselbund. 4. Tabelle A3. 5. Ggf. Umwandlung
(A6). 6. Öffnen, Lesbarkeit, Migrationen; danach einmalig die Umstellung der
früher feldweise verschlüsselten Inhalte (Spec 0036, Abschnitt 3).
7. Secrets-Umzug (A10–A11.1). 8. MCP-Token (A12). 9. Übriger Zustand, dann
MCP-Server.

- K ist die Wurzel des Datenbankschlüssels. Der Modus ergibt sich aus der
  Existenz der Verpackungsdatei.
- Zwischen-, Verpackungs- und umbenannte Dateien liegen im Datenverzeichnis,
  unter Unix mit Rechten 0600.
- Secrets-Tabelle und Umzugszustand liegen in der Datenbank; Secrets sind dort
  nicht zusätzlich feldweise verschlüsselt.
- Der Zustand entsteht vor dem ersten Fenster, außer im Passwort-Modus
  (Frage 3).

## 6. Sicherheitszusagen

- **Credential-Speicher:** gleiche Semantik wie der Schlüsselbund-Speicher
  (A9); Spec 0071 A14/I4 gilt für beide; Spec 0098 für den Zugriff auf K.
- **Fehlerpfade im UI** (Spec 0059): Jeder Fall aus A3, A6, A11, A11.1, A16,
  A17 hat einen Dialog oder eine Meldung, keiner endet still.
- **Keine stillen Rückfälle:** kein Weiterlauf im Klartext, kein neuer K
  außer in den Fällen aus A3, kein Wechsel des Modus ohne Nutzerhandlung.
- **Log und Redaction** (Spec 0094): keine Schlüssel, Passwörter oder Secrets
  in Log, Diagnosepaket oder DTO; das MCP-Token nicht in Log und
  Diagnosepaket und persistiert nur in der verschlüsselten Datenbank
  (Klarstellung 8); neue Log-Zeilen mit festen Texten.
- **Datensenken:** Secrets-Tabelle (nur in der verschlüsselten Datei),
  Verpackungsdatei (nur Chiffrat), Zwischendatei (verschlüsselt).
- **MCP:** Token-Wert bleibt bei der Übernahme gleich; vor der Entsperrung
  nicht erreichbar.

## 7. Testfälle

Grundlage: der Rohdatei-Test aus Spec 0096 (Datei, `-wal`, `-journal`).
Marker je Test eindeutig: `host-0101.example`, `user-0101`, `Header-0101`,
`Secret-0101`, `Token-0101`.

- **T0 (Fixture):** Datenbankdatei vom Build vor dieser Spec, 14
  Migrationen, Beispielzeilen mit allen Markern, feldweise verschlüsselter
  Chatinhalt unter einem festen Test-K. Grundlage für T4–T6.
- **T1 (A1–A2, Rohdatei):** Neue Installation; Server, Provider mit Header,
  Secret, MCP-Token anlegen, Verbindung offen → kein Marker, kein
  Klartext-Header in Daten- und Konfigurationsverzeichnis.
- **T2 (A2):** Known-Answer: festes K → fester Hex-Schlüssel; verschiedene K
  → verschiedene Schlüssel, Schlüssel ≠ K.
- **T3 (A3/A4, Tabelle):** je Feld der Tabelle ein Fall mit Test-Speicher,
  der Aufrufe zählt: erwarteter Ausgang, Schreiben von K ohne Nutzerwahl nur
  in den zwei „K erzeugen"-Feldern, Datei in allen Dialog-Fällen byte-gleich,
  solange nichts gewählt ist, nie Code 7.
- **T4 (A6, A7):** Fixture umwandeln → Zeilen je Tabelle gleich,
  `user_version` gleich, `journal_mode=wal`, Migrationen vollständig, kein
  Marker, kein Klartext-Original, Chatinhalt mit Test-K lesbar.
- **T5 (A6, Abbruch):** Fehlerinjektion nach Schritt 2 und nach Schritt 3 →
  Original inhaltsgleich, keine Zwischendatei, nächster Start wandelt um.
- **T6 (A6, WAL):** Zeile nur im WAL des Originals → nach der Umwandlung
  vorhanden; keine alte `-wal` neben der neuen Datei (Klarstellung 7).
- **T7 (A5, D4):** „Neu anfangen" aus D2 und D3 → Dateien umbenannt,
  byte-gleich, Dialogtext nennt den Namen, neue leere Datenbank; ohne zweite
  Bestätigung passiert nichts. D4 → Klartext-Datei mit neuem K umgewandelt,
  Zeilen erhalten. Passwort-Modus für A5 und D4: Einrichten abgebrochen →
  alle Dateien unverändert; durchgeführt → alte Verpackungsdatei umbenannt
  (nicht überschrieben), kein Schreiben in den Schlüsselbund.
- **T8 (D1):** K nicht erreichbar → Datenbank nicht geöffnet (Inhalt und
  mtime gleich); „Erneut versuchen" mit danach funktionierendem Speicher
  startet.
- **T9 (A1):** Drittlizenz-Ausgabe enthält SQLCipher und OpenSSL.
- **T10 (A9, A9.1):** Vertragstests des Speichers (Lesen nach Schreiben,
  `NotFound`, idempotentes Löschen, Überschreiben); Fehler ergibt
  `SECRET_STORE_FAILED` ohne Secret, auch bei nicht verfügbarem
  Schlüsselbund. Verbindungstest mit Jump-Host, dessen Secret-Lesen am neuen
  Speicher scheitert → `SECRET_STORE_FAILED`, kein `KEYCHAIN_*`, kein
  „Schlüsselbund".
- **T11 (A10–A11.1):** Test-Schlüsselbund mit zwei Servern (alle Slots) und
  einem Provider, ein Slot fehlt → alles umgezogen, Slot bleibt `NotFound`,
  alle Einträge gelöscht. Varianten: ein Lesen scheitert → nichts gelöscht,
  Zustand *offen*; Löschen scheitert → Start, nächster Start löscht; Server
  gelöscht, während Löschen aussteht → seine Einträge werden trotzdem
  gelöscht; *übersprungen*, danach Schlüsselbund erreichbar → kein einziges
  Löschen; Schlüsselbund-Modus mit scheiterndem Lesen → keine Option „Ohne
  Übernahme".
- **T12 (A12):** Einstellungsdatei mit Token → Token in der Datenbank,
  gleicher Wert, Schlüssel dort entfernt; Erneuern schreibt nicht dorthin.
- **T13 (A13–A15):** Einrichten → Schlüsselbund ohne K, Verpackungsdatei da;
  Passwort mit 11 Zeichen und ohne Bestätigung abgelehnt; Fehlerinjektion
  nach dem Schreiben der Verpackung → K bleibt im Schlüsselbund. Neustart
  mit richtigem Passwort öffnet; Passwort ändern → altes scheitert, neues
  gelingt, Datenbank byte-gleich; zurück auf Schlüsselbund →
  Verpackungsdatei weg. Einrichten aus D1 bei Klartext-Datei → umgewandelt,
  K nur verpackt. Verpackungsdatei unter Unix mit Rechten 0600. D1 bietet
  **kein** Einrichten bei `Locked`/`Unknown`/Backend-Fehler und bei Datei
  *sonst* × `NoSecretServiceProvider`/`NoSessionBus`; der Umzugs-Dialog
  (A11) nie.
- **T14 (A16, A18, A20):** Oberfläche: Entsperrmaske, falsches Passwort
  zeigt Meldung, Modusanzeige, Einrichten mit Bestätigung, die Codes
  übersetzt.

Adversarial (ERHÖHT):

- **T15 Verpackung manipuliert:** ein Byte in Chiffrat, Salt oder Kopf
  geändert → Entsperren scheitert, kein Zugriff, nichts gelöscht. Gültige
  Verpackung mit m = 8 KiB → abgelehnt.
- **T16 Halb abgebrochener Wechsel (A17):** Verpackung und Eintrag, einmal
  gleich, einmal verschieden → Verpackung gilt; gelöscht nur im Gleich-Fall.
- **T17 Schlüssel in Ausgaben:** Log und Diagnosepaket nach T1, T3, T11, T13
  ohne K (Hex/Base64), Datenbankschlüssel, Passwort, Marker.
- **T18 Kommando vor Entsperrung:** Daten-Kommando und MCP-Anfrage im
  gesperrten Zustand → Fehler, kein Panic, keine Daten.
- **T19 Fremde Zwischendatei:** fremde Datei am Ort der Zwischendatei →
  verworfen, nicht als fertige Umwandlung übernommen.
- **T20 Symlink:** `smart-ssh.db` ist Symlink auf eine Klartext-Datei →
  Startfehler, Ziel und Symlink unverändert, keine verschlüsselte Datei
  angelegt.

## 8. Grenzen

- Wer Zugriff auf das entsperrte Benutzerkonto hat, kann die Daten lesen.
- Ohne Master-Passwort oder Schlüsselbund-Schlüssel gibt es keine
  Wiederherstellung (E10).
- Alter Klartext kann in Backups und Schnappschüssen liegen bleiben.

## 9. Klarstellungen

1. **Erststart-Hinweis (A21):** DE wörtlich: „Die lokale Datenbank ist
   verschlüsselt. Den Schlüssel verwahrt der Schlüsselbund deines
   Betriebssystems oder – wenn du es einrichtest – dein Master-Passwort. Wer
   Zugriff auf dein entsperrtes Benutzerkonto hat, kann die Daten lesen."
   EN sinngemäß, gleicher Inhalt.
2. **Fixture unter Windows:** Windows-Builds checken die Migrationen mit CRLF
   aus; die Prüfsummen in der Migrationstabelle unterscheiden sich dort von
   der unter LF geschriebenen Fixture. Tests, die die Fixture öffnen (T0,
   T4–T6), setzen in ihrer **Kopie** vor dem Öffnen die Prüfsummen auf die
   des laufenden Builds; die eingecheckte Datei bleibt unverändert. Im Feld
   tritt der Fall nicht auf. Der Produktivcode hat dafür keine
   Sonderbehandlung.
3. *(entfallen; Inhalt in Abschnitt 1 und 2.)*
4. **A2, Schreibweise:** Der rohe Schlüssel steht in der Pragma-Anweisung in
   doppelten Anführungszeichen, `"x'<64 Hex>'"` (Abschnitt 1).
5. **A3 D1, Verlusthinweis:** Der Hinweis auf den Verlust des bisherigen
   Verlaufs gehört zum Knopf „Master-Passwort einrichten", denn nur diese Wahl
   erzeugt einen neuen K. D1 ohne diesen Knopf zeigt ihn nicht (ADR 0093 §4).
6. *(entfallen; betraf die Aufteilung der Umsetzung.)*
7. **T6 unter Windows:** Hält der Test die Klartext-Verbindung im selben
   Prozess offen, sperrt Windows das Umbenennen. T6 kopiert deshalb nach dem
   Schreiben Datenbank, `-wal` und `-shm` in ein zweites Verzeichnis und
   wandelt die **Kopie** um. Eine zweite laufende Instanz bricht die
   Umwandlung weiterhin ab.
8. **MCP-Token in der Anzeige:** Das Einstellungs-DTO für den MCP-Server
   liefert das Token weiterhin an die Oberfläche, denn Spec 0028 §9 verlangt
   seine Anzeige. Abschnitt 6 verbietet das Token in Log, Diagnosepaket und
   in jeder Speicherung außerhalb der verschlüsselten Datenbank, nicht in
   diesem DTO.
9. **Passwort-Modus (Etappe 3):**
   - Eine unbrauchbare Verpackungsdatei (Authentifizierung scheitert
     dauerhaft, Kopf oder Parameter ungültig) führt in die Spalte *ungültig*
     der Tabelle A3; „Neu anfangen" ist vor der Entsperrung erreichbar
     (A16). Falsches Passwort bleibt erneute Eingabe; „Neu anfangen" ist
     immer eine ausdrückliche Nutzerwahl.
   - A16 gilt für **alle** Kommandos, auch die der Plugins: Vor der
     Entsperrung ist kein Plugin-Kommando erreichbar, das Dateien,
     Einstellungen oder das Betriebssystem berührt. Einstellungsdateien sind
     vor der Entsperrung nicht lesbar.
   - Passwort ändern und Einrichten prüfen die neue Verpackung, **bevor** sie
     die alte ersetzt bzw. K aus dem Schlüsselbund entfernt; Reste einer
     abgebrochenen Verpackung bleiben nicht liegen; nach dem Umbenennen wird
     das Verzeichnis synchronisiert.
   - Entsperren läuft höchstens einmal gleichzeitig; ein Startdialog, dessen
     Antwort ausbleibt, scheitert sichtbar statt zu hängen (Spec 0059).
   - Fehlertexte stimmen mit dem Zustand überein, den sie beschreiben;
     Einrichten aus den Einstellungen nutzt den K des offenen Zustands.
   - A19 gilt auch für K im Entsperrergebnis.
10. **Passwort-Modus, zweiter Teil:**
    - a. Die Verpackungsdatei wird nach Fehlerart eingeordnet wie der
      Schlüsselbund in A3: Fehlt die Datei oder lässt sie sich nicht lesen
      (Rechte, E/A-Fehler, von einem anderen Programm gesperrt), gilt sie als
      *nicht erreichbar* (D1, nichts verändern). *Ungültig* ist sie nur, wenn
      sie gelesen wurde und Format, Kopf oder Parameter nicht stimmen.
      Dauerhaft gescheiterte Authentifizierung bleibt „Passwort falsch oder
      Datei beschädigt" (A17) mit „Neu anfangen" als ausdrücklicher Wahl.
    - b. Wechsel Passwort → Schlüsselbund (A15): Liegt im Schlüsselbund
      bereits ein Eintrag, der nicht gleich K ist, fragt die Anwendung vor dem
      Überschreiben: „Im Schlüsselbund liegt ein anderer Schlüssel. Backups,
      die mit diesem Schlüssel verschlüsselt sind, werden danach unlesbar.
      Ersetzen?" (EN sinngemäß). Ohne ausdrückliche Bestätigung bleibt alles,
      wie es war. Ist der Eintrag gleich K, entfällt die Frage.
    - c. Die verzögerte Registrierung der Plugins im Passwort-Modus ist
      getestet.
    - d. T17 deckt alle genannten Pfade ab, nicht nur den Passwort-Modus.
    - e. Ein reiner Hinweis an den Nutzer wartet auf keine Antwort; ein
      geöffnetes Passwortfeld ist leer.
11. **„Neu anfangen" bei dauerhaft gescheiterter Authentifizierung:** Nach
    drei gescheiterten Entsperrversuchen im selben Programmlauf bietet die
    Entsperrmaske „Neu anfangen" an, mit dem Text aus A5 („Verlauf ist danach
    verloren, die Dateien werden nur umbenannt") und einer zweiten
    Bestätigung; im Passwort-Modus zuerst das neue Passwort. Der Zähler liegt
    nur im Speicher und beginnt bei jedem Start bei null; vor der
    Entsperrung wird nichts dafür geschrieben. Vor dem dritten Fehlversuch
    gilt Klarstellung 9. Ist die Verpackungsdatei *nicht erreichbar*, zeigt
    die Maske „Erneut versuchen" mit einem Hinweis auf die Datei und kein
    „Neu anfangen".
12. **Bestätigung aus A13/E10 im Backend:** Wie Länge und Wiederholung prüft
    auch das Backend, dass die Warnung ausdrücklich bestätigt wurde. Jeder
    Weg, der ein Master-Passwort einrichtet, lehnt ohne diese Bestätigung ab
    und verändert nichts.
