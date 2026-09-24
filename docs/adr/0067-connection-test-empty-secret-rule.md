# ADR 0067: Verbindungstest und Speichern bewerten leere Zugangsdaten gleich

Status: Angenommen
Bezug: docs/specs/0073-shared-credential-trim.md (§9, Q-BL-0149-02),
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

**Warum beides zusammen und nicht nur der Trim:** Ein reiner Trim ohne die
Leer-Regel hätte den Fehlerfall **häufiger** gemacht, weil ein Leerraum-
oder BOM-Paste dann auf `""` fällt und vorher genau dieses `""` in den
Anmeldeversuch ging. Halbherzig nachzuziehen wäre hier schlechter gewesen
als gar nicht — deshalb trug die Frage eine Option, die beides ändert.

## 2. Was sich für den Nutzer sichtbar ändert

Nur ein Fall, und der war vorher irreführend: Bei **Neuanlage** mit leerem
Pflichtfeld endet „Verbindung testen" jetzt in der Fehlermeldung statt in
einem Anmeldeversuch mit leerem Secret. Auf einem Server mit
`PermitEmptyPasswords yes` meldete der Test bisher Erfolg für ein Profil,
das sich anschließend nicht speichern ließ.

Beim **Bearbeiten** ändert sich nichts an der Bedeutung: leer heißt
weiterhin „das hinterlegte Credential verwenden" (Spec 0008 §4). Neu ist
nur, dass auch ein Leerraum- oder Nur-unsichtbare-Zeichen-Paste als leer
gilt und nicht mehr das hinterlegte Credential durch einen Wert ersetzt,
den der Nutzer nicht gemeint hat.

## 3. Keine Lockerung

Die Änderung verschärft in beide Richtungen und lockert nichts:

- Ein Wert, der vorher akzeptiert wurde und nicht leer ist, wird weiter
  akzeptiert — höchstens ohne seine Randzeichen (I3 der Spec: der Helfer
  entfernt mindestens das, was `str::trim` entfernt hätte, und `resolve_secret`
  trimmte vorher überhaupt nicht).
- Ein Wert, der vorher zu einem Anmeldeversuch mit leerem Secret führte,
  führt jetzt zum Fehler oder zum hinterlegten Credential. Es entsteht
  kein Pfad, auf dem ein Anmeldeversuch **mehr** darf als vorher.
- Die Fehlermeldung bleibt die vorhandene („Secret erforderlich (kein
  bestehender Server zum Wiederverwenden gefunden)"), ohne neuen
  Fehlercode. Absicht: Die Entscheidung betrifft, **wann** abgebrochen
  wird, nicht wie der Abbruch nach außen aussieht.
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
