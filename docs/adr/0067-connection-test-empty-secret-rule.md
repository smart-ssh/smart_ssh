# ADR 0067: Verbindungstest und Speichern bewerten leere Zugangsdaten gleich

Status: Angenommen
Bezug: docs/specs/0073-shared-credential-trim.md (§9, Q-BL-0149-02,
Q-BL-0149-03),
ADR 0066 §3 (dort als offen protokolliert), Spec 0049 (Fund 1),
Spec 0008 (§4, §7)

ADR 0066 §3 hat einen Punkt offen gelassen: `test_connection::resolve_secret`
reichte einen eingegebenen Wert ungetrimmt weiter und nahm **jeden**
`Some`-Wert, auch `""`, während `write_or_reuse_secret` beim Speichern
trimmt und einen leeren Wert als „unverändert lassen" bzw. als Fehler
bewertet. Dieses ADR hält fest, wie der Punkt entschieden wurde und was
dabei bewusst **nicht** geändert wurde.

## 1. Die Entscheidung

Q-BL-0149-02, Option 2b. `resolve_secret` bekommt dieselbe Gestalt wie
`write_or_reuse_secret`:

```rust
match provided.map(|value| trim_credential_value(&value)) {
    Some(value) if !value.is_empty() => /* dieser Wert */,
    _ => /* gespeichertes Credential des bestehenden Servers, sonst Fehler */,
}
```

Gültig für `Password`, `PrivateKey.key_content`, `Certificate.cert_content`
und `Certificate.key_content`. Die Passphrase lief schon vorher so
(ADR 0066 §2), damit ist der Verbindungstest in allen Secret-Slots
gleichförmig.

**„Dieselbe Gestalt" heißt dieselbe Entscheidungsregel, nicht
Verhaltensgleichheit in jedem Fall.** Ein Unterschied bleibt und ist
gewollt: Beim Speichern genügt die Feststellung, dass der Slot derselben
Anmeldeart vorher existierte (`previously_existed`); der Verbindungstest
muss das Secret zusätzlich **lesen**, weil er sich damit anmeldet. Ist der
Schlüsselbund-Eintrag von außen verschwunden, ist Speichern deshalb `Ok`
und der Test `Err`. Das galt für ein `None`-Feld schon vor diesem Diff und
ist die richtige Richtung: Der Test soll nicht behaupten, er habe ein
Credential benutzt, das er nicht bekommen hat.

**Warum beides zusammen und nicht nur der Trim:** Ein reiner Trim ohne die
Leer-Regel hätte den Fehlerfall **häufiger** gemacht, weil ein Leerraum-
oder BOM-Paste dann auf `""` fällt und vorher genau dieses `""` in den
Anmeldeversuch ging. Halbherzig nachzuziehen wäre hier schlechter gewesen
als gar nicht — deshalb trug die Frage eine Option, die beides ändert.

## 2. Was sich für den Nutzer sichtbar ändert

Der Fall, um den es ging, war vorher irreführend: Bei **Neuanlage** mit
leerem Pflichtfeld endet „Verbindung testen" jetzt in der Fehlermeldung
statt in einem Anmeldeversuch mit leerem Secret. Auf einem Server mit
`PermitEmptyPasswords yes` meldete der Test bisher Erfolg für ein Profil,
das sich anschließend nicht speichern ließ.

Beim **Bearbeiten** ändert sich nichts an der Bedeutung: leer heißt
weiterhin „das hinterlegte Credential verwenden" (Spec 0008 §4). Neu ist
nur, dass auch ein Leerraum- oder Nur-unsichtbare-Zeichen-Paste als leer
gilt: Der Test meldet sich dann mit dem hinterlegten Credential an, statt
mit einem Wert, den der Nutzer nicht gemeint hat. Ersetzt wird dabei
nichts — der Verbindungstest liest nur (`real_store.get`), er schreibt
kein Credential und löscht keines.

Das ist die einzige Richtung, in der eine Erfolgsmeldung nach diesem Diff
über einen Wert aussagt, den der Nutzer **nicht** eingegeben hat: Ein
Verbindungstest, der vorher an einem Leerraum-Paste scheiterte, kann jetzt
grün werden — mit dem hinterlegten Zugangsdatum. Genau das tut „Speichern"
seit Spec 0008 §4 auch, und §9 verlangt die Gleichstellung ausdrücklich.
Festgehalten sei es trotzdem.

Ein zweiter Fall geht ebenfalls von rot auf grün, ist aber harmlos: Ein
**nicht** leerer Paste mit Randzeichen scheiterte vorher an der
Authentifizierung und geht jetzt durch — mit dem getrimmten **eigenen**
Wert des Nutzers, nicht mit einem fremden. Das ist der Zweck der Spec
(§3 erster Punkt unten).

## 3. Keine Lockerung

Die Änderung verschärft in beide Richtungen und lockert nichts:

- Ein Wert, der vorher akzeptiert wurde und nicht leer ist, wird weiter
  akzeptiert — höchstens ohne seine Randzeichen (I3 der Spec: der Helfer
  entfernt mindestens das, was `str::trim` entfernt hätte, und `resolve_secret`
  trimmte vorher überhaupt nicht).
- Ein Wert, der vorher zu einem Anmeldeversuch mit leerem Secret führte,
  führt jetzt zum Fehler oder zum hinterlegten Credential. Es entsteht
  kein Pfad, auf dem ein Anmeldeversuch **mehr** darf als vorher.
- Wie der Abbruch nach außen aussieht, hat Q-BL-0149-03 nachträglich
  entschieden (§6); die Entscheidung dieses ADR betrifft, **wann**
  abgebrochen wird, und blieb dabei unberührt.
- Kein Secret gelangt in eine Meldung oder ein Log (I2). Getrimmt wird im
  Kern, die Meldung nennt nur den Slot.

## 4. Die Tests

Drei Tests vergleichen je Slot die **Ergebnisse** beider Wege, nicht ob
beide denselben Funktionsnamen aufrufen
(`crates/app-shell/src/test_connection.rs`, `secret_both_ways`):

1. Paste mit Rand-Leerraum, BOM und Zero-Width-Space → beide Wege benutzen
   dasselbe getrimmte Secret.
2. Leeres Feld beim Bearbeiten (`""`, Leerraum, nur unsichtbare Zeichen) →
   beide Wege benutzen das hinterlegte Credential.
3. Dasselbe bei Neuanlage → beide Wege brechen ab.

Der Ausgangszustand „bestehender Server" entsteht durch einen echten
Speicher-Vorgang, damit die Credential-Refs genau die sind, die
`update_server` wiederverwendet — und nicht von Hand gebaute, die nur
zufällig passen.

**Gegenbeweis:** Alle drei scheitern gegen den Stand vor dem Fix; der
gemessene Ist-Wert war jeweils `Ok("")` bzw. der ungetrimmte Paste.

Dazu zwei Zusicherungen, die nicht den Fix prüfen, sondern die Zusage des
Moduls „ohne irgendetwas zu persistieren" festnageln:

- Der echte Store muss nach dem Test-Weg Inhalt für Inhalt so aussehen wie
  davor. Verglichen wird der **ganze** Store, nicht eine Liste erwarteter
  `test:*`-Refs — sonst deckt die Prüfung den Neuanlage-Fall nicht ab, in
  dem es kein hinterlegtes Credential gibt, und auch kein Schreiben auf
  einen Nachbar-Slot. Gegenbeweis gemessen: ein vorübergehend eingebautes
  `set` auf `server:sabotage:certificate_key` lässt damals alle drei Tests mit
  dieser Meldung scheitern.
- Der Neuanlage-Test prüft, dass der Abbruch der fehlenden Eingabe gilt,
  nicht irgendeinem anderen Grund — über den `code` des Fehlers (§6),
  nicht über den Anfang des Textes. Der Text ist nur der Rückfall ohne
  Übersetzung; der Code ist die Schnittstelle zum Frontend.

## 5. Was offen bleibt

Die in ADR 0066 aufgeführten Punkte bleiben unverändert offen — ADR 0064
§2 (Zeichenliste), §3 (`model`), §4 (`add_ai_provider`) und §7
(u. a. `Zeroize` auf dem Zwischen-`String`). Dieses ADR räumt allein
ADR 0066 §3 ab.

Zusätzlich vorgemerkt, von diesem Diff nicht berührt und nicht Gegenstand
dieser Spec:

- **`dto.rs:622`** echot einen abgelehnten `sftp_server_path` wörtlich in
  eine UI-Meldung, die geloggt werden kann; ein Steuerzeichen im Wert
  kommt so mit. Kein Secret, deshalb kein Redaction-Fall, aber ein
  eigener Backlog-Punkt wert.
- **Credential-Verlust bei fehlgeschlagenem Methodenwechsel**
  (`server_credentials.rs:298` mit `commands.rs:2581`).
  `resolve_auth_method` ruft `cleanup_abandoned_slots` **vor** der
  Pflichtfeld-Prüfung, und `update_server` bricht danach mit `?` ab, ohne
  das Gelöschte wiederherzustellen: Wer bei einem bestehenden Server mit
  Passwort auf „Zertifikat" umstellt, die Felder leer lässt und speichert,
  sieht „Feld fehlt" — das alte Passwort ist aber aus dem Schlüsselbund
  weg, während die Datenbank weiter `AuthMethod::Password` trägt. Ein
  Löschen ohne Bestätigung und ohne Sichtbarkeit. Von diesem Diff nicht
  verursacht und nicht berührt; der Weg liegt aber genau dort, wo §1 über
  „Schlüsselbund-Eintrag verschwunden" argumentiert — erreichbar also auch
  von innen. Eigener Backlog-Punkt mit eigener Spec-Klärung (Reihenfolge
  umdrehen oder Rollback wie in `servers.rs:44-50`); der Befund ist aus
  Codelesen hergeleitet, nicht im laufenden Programm gemessen.
- **Leerer `identityFile.path` im Verbindungstest** endet in einem
  Lesefehler des `KeyFileReader` (`NetworkError`) statt in
  `SERVER_IDENTITY_FILE_REQUIRED` wie beim Speichern. Dieselbe Art
  Divergenz wie Q-BL-0149-02, aber der Pfad ist kein Secret und führt zu
  keinem falschen Erfolg. Von Q-BL-0149-03 (§6) nicht abgedeckt.
- **`expose`s Panic-Text im Testmodul** sagt „Passphrase-Slot muss
  auflösbar sein", wird inzwischen aber auch für Passwort-, Zertifikats-
  und Key-Slots benutzt. Einzeiler, vorbestehend, irreführend nur im
  Fehlerfall eines Tests.

## 6. Der Fehler trägt die Codes des Speicherns (Q-BL-0149-03)

Entschieden (Q-BL-0149-03, Option 1): `resolve_secret` bekommt einen
Parameter `code: &'static str`, wie ihn `write_or_reuse_secret` schon hat.
Die vier Aufrufe geben `SERVER_PASSWORD_REQUIRED`,
`SERVER_PRIVATE_KEY_REQUIRED`, `SERVER_CERTIFICATE_REQUIRED` und
`SERVER_CERTIFICATE_KEY_REQUIRED` mit. Beide Knöpfe zeigen damit bei
derselben Eingabe denselben übersetzten Satz. Das Frontend, die
Übersetzungen und `keychain_aware_credential_error` bleiben unverändert;
der Schlüsselbund-Zweig (`KEYCHAIN_UNAVAILABLE`) ist nicht berührt.

- **Der Text** (`Secret erforderlich (Feld leer, kein hinterlegtes
  Credential dieser Anmeldeart)`) ist nur noch der Rückfall, falls ein
  Code einmal keine Übersetzung findet. Der frühere Wortlaut „kein
  bestehender Server zum Wiederverwenden gefunden" war für einen
  bestehenden Server mit **anderer** Anmeldeart falsch (gespeichert
  `Agent`, im Formular auf „Passwort" umgestellt, Feld leer) und ist
  deshalb nicht beibehalten worden.
- **Keine Lockerung:** Abgebrochen wird an derselben Stelle und unter
  denselben Bedingungen wie zuvor; geändert sind allein `code` und Text
  des Fehlers. Weder der eingegebene noch ein hinterlegter Wert gelangt in
  die Meldung (I2). Der Test dazu
  (`test_q0149_03_refusal_message_carries_no_secret`) kann heute nicht
  scheitern — die Meldung ist ein festes Literal —, er schlägt erst an,
  wenn jemand einen Wert in sie hineinformatiert; er prüft nur den
  hinterlegten Wert eines Nachbar-Slots, ein eingegebener nicht leerer
  Wert führt nie in diesen Zweig.
- **Tests:** je Slot der `code` gegen ein Literal im Test (nicht aus dem
  Speicher-Weg abgeleitet), bei Neuanlage und bei „bestehender Server mit
  Agent bzw. anderer Secret-Anmeldeart, Feld leer" — jeweils für den
  Verbindungstest **und** das Speichern.
- **Gegenbeweis (gemessen):** Gegen den Stand `5051e68` (Fehler ohne
  `code`) scheitern beide Code-Tests beim ersten Slot mit `code: None`.
  Ein Code, der an einer Aufrufstelle mit dem des Nachbar-Slots vertauscht
  wird (Zertifikats-Key meldet `SERVER_CERTIFICATE_REQUIRED`), lässt
  dieselben Tests ebenfalls scheitern.
- **Zurückgestellt (Review Runde 5):** Kein Frontend-Test belegt, dass
  `runTest` einen `SERVER_*_REQUIRED`-Fehler übersetzt anzeigt — die Kette
  ist durch Lesen von `ServerForm.tsx` und `errorCodes.ts` belegt, nicht
  durch einen Test. Der Rückfalltext des Test-Wegs weicht vom Rückfalltext
  des Speicherns („Passwort ist erforderlich") ab; in der Oberfläche
  unsichtbar, da alle vier Codes übersetzt sind. Ein extern gelöschter
  Schlüsselbund-Eintrag lässt den Test weiter ohne Code scheitern (§1).
- **Nicht Teil dieser Entscheidung:** ob „Verbindung testen" bei leeren
  Pflichtfeldern gar nicht erst anlaufen soll (Frontend-Frage), und der
  leere `identityFile.path` (§5).
