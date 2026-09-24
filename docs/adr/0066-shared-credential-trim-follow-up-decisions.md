# ADR 0066: Nachlauf zu Spec 0073 — Pfad-Override und Passphrase im Verbindungstest

Status: Angenommen
Bezug: docs/specs/0073-shared-credential-trim.md (§9 Klarstellungen),
ADR 0064, Commits `2956e57` (Pfad-Override), `cb07f0c` (Passphrase im
Verbindungstest)

ADR 0064 hat mehrere Punkte offen protokolliert. Dieses ADR räumt **einen**
davon ab (0064 §1, Annahme A-1), hält eine Klarstellung fest, die es dort
noch nicht gab (BL-0243), und legt einen Punkt offen, den 0064 nicht gesehen
hat (Q-BL-0149-02).

**Unverändert offen und ausdrücklich nicht Gegenstand dieses ADR:**

- **ADR 0064 §2** — die Zeichenliste aus A1 ist unvollständig
  (U+200E/200F, U+202A–202E, U+2066–2069, U+00AD stehen nicht darin).
  Produktentscheidung, nicht getroffen.
- **ADR 0064 §3** — `model` wird nicht getrimmt. Offen gelassen, nicht
  vorentschieden.
- **ADR 0064 §4** — `add_ai_provider` legt bei einem `api_key` aus lauter
  unsichtbaren Zeichen still einen Provider ohne Credential an. Bewusst
  nicht behoben (ein Fehler wäre neues Verhalten bei leerer Eingabe,
  Spec 0073 §2 Nicht-Ziel 3), vorgemerkt als Backlog-Punkt.
- **ADR 0064 §7** — die dort vorgemerkten Punkte, darunter das fehlende
  `Zeroize` auf dem Zwischen-`String` von `trim_credential_value`.

Wer hier liest, hat damit noch nicht gelesen, was von Spec 0073 offen ist;
dafür ist ADR 0064 zu lesen, u. a. §2, §3, §4 und §7.

## 1. `normalize_sftp_server_path` benutzt den Helfer (A-1 aufgelöst)

**Vorgeschichte:** ADR 0064, Abschnitt 1 — die Stelle steht in der
§1-Tabelle der Spec, ist aber kein Zugangsdaten-Wert, sondern ein
Pfad-Override, der in ein `sudo`-Kommando wandert. Der Coder hat sie
deshalb nicht umgestellt, sondern als `ANNAHME A-1 (Q-BL-0149-01)`
markiert und gefragt.

**Entscheidung (Q-BL-0149-01, Option 1, protokolliert in Spec 0073 §9):**
Die Stelle benutzt `trim_credential_value`. Die Annahme und ihre
Code-Markierung sind entfallen.

**Was sich dadurch ändert — und was ausdrücklich nicht:**

- `is_plausible_sftp_server_path` ist **wörtlich unverändert** und läuft
  weiterhin *nach* dem Trimmen, auf genau dem Wert, der zurückgegeben und
  gespeichert wird. Keine Prüfung wurde gelockert, keine übersprungen.
- Der Helfer entfernt nur Randzeichen (Spec 0073, A2/I1). Ein unsichtbares
  Zeichen **innerhalb** des Pfades überlebt ihn und fällt danach durch die
  unveränderte Prüfung. In ein `sudo`-Kommando kann also weiterhin kein
  Zeichen außerhalb des erlaubten Zeichensatzes
  (`[A-Za-z0-9/._+-]`, absolut, kein `..`, Dateiname `sftp-server`)
  gelangen.
- Geändert hat sich genau eines: ob ein eingefügter Pfad mit einem
  unsichtbaren Randzeichen als **Fehler** endet oder als **bereinigter
  Pfad**. Und, als Folge davon, dass eine Eingabe aus *lauter*
  unsichtbaren Zeichen jetzt `Ok(None)` = „automatisch" ergibt statt eines
  Fehlers — dieselbe Semantik, die `"   "` schon vorher hatte. Diese
  Nebenwirkung war in ADR 0064 benannt und ist so gewollt.

Drei Tests halten das fest (`dto.rs`, `sftp_server_path_tests`): Rand wird
bereinigt · Zeichen innen wird weiterhin abgelehnt · nur unsichtbare
Zeichen ergeben `None`. Der erste und der dritte scheitern gegen den Stand
vor der Umstellung; der zweite ist bewusst ein **Bewahrungstest** — er war
auch vorher grün und soll genau das bleiben.

## 2. Die Passphrase im Verbindungstest zieht nach (BL-0243)

**Befund:** „Verbindung testen" und „Speichern" lesen dasselbe
Formularfeld, behandelten es aber verschieden.
`server_credentials::resolve_auth_method` schickt die Passphrase seit
Spec 0073 A3 durch `trim_credential_value`, mit der Regel „nach dem
Trimmen leer = die hinterlegte Passphrase behalten".
`test_connection::resolve_final_hop_auth` tat das nicht: Im
`PrivateKey`-Zweig ging der Wert **unverändert** in den Anmeldeversuch,
im `IdentityFile`-Zweig diente `str::trim` nur als Leer-Prüfung, gelegt
wurde trotzdem der rohe Wert.

**Entscheidung (K1 des Architekten, Spec 0073 §9):** Beide Zweige benutzen
den geteilten Helfer.

**Über den Wortlaut der Klarstellung hinaus — und warum:** Die Klarstellung
verlangt, dass „dieselbe Passphrase in beiden Wegen zum selben Ergebnis
führt". Das ist mit dem Helfer allein nicht erreicht: Solange der
Testpfad einen nach dem Trimmen leeren Wert als „neue Passphrase" wertet
und der Speicherpfad als „unverändert lassen", gehen dieselbe Eingabe und
derselbe Server weiter auseinander. Der Testpfad hat deshalb auch die
Leer-Regel übernommen und fällt in diesem Fall auf das hinterlegte
Credential zurück — genau wie das Speichern. Ohne diesen Teil hätte die
Klarstellung ihr eigenes Ziel verfehlt.

**Richtung der Änderung:** Sie verschärft nichts und lockert nichts am
Filter- oder Eskalationspfad; sie betrifft nur, mit welchem Wert der
Verbindungstest anmeldet. Kein Secret wird dabei zusätzlich persistiert:
Der Wert landet wie zuvor ausschließlich im `EphemeralCredentialStore`,
der mit dem Aufruf endet.

Drei Tests vergleichen die **Ergebnisse** beider Wege je Anmeldeart mit
Passphrase (`PrivateKey`, `IdentityFile`) — nicht, ob beide Stellen
denselben Funktionsnamen aufrufen. Alle drei scheitern gegen den Stand
vor dem Fix.

## 3. Offen: Passwort und Key-Inhalt im Verbindungstest (Q-BL-0149-02)

> **Nachtrag 2026-09-24: entschieden und umgesetzt, s. ADR 0067.** Stefan
> hat Option 2b gewählt (Spec 0073 §9): `resolve_secret` trimmt über den
> geteilten Helfer und wendet dieselbe Leer-Regel an wie das Speichern. Der
> folgende Abschnitt beschreibt den Stand **vor** dieser Entscheidung und
> bleibt als Begründung stehen, warum es zwei Läufe gebraucht hat.

Dieselbe Divergenz besteht für die übrigen Slots des Verbindungstests.
`test_connection::resolve_secret` reicht einen eingegebenen Wert
ungetrimmt weiter, während `write_or_reuse_secret` beim Speichern trimmt —
betroffen sind `Password`, `PrivateKey.key_content` sowie Zertifikat und
Zertifikats-Key.

Die Klarstellung K1 nennt ausdrücklich nur die Passphrase, und eine
Änderung am Anmeldepfad für Passwörter ist eine Produktentscheidung, keine
Ableitung. Deshalb **nicht** mitgeändert, sondern als Q-BL-0149-02
vorgelegt (Klasse K3, offen). Der Stand der Frage steht im
Abschlussbericht dieses Laufs.

**Der Trim ist dabei der kleinere Teil.** Beim Einordnen der Frage ist
aufgefallen, dass die beiden Wege sich schon in der Bedeutung von „leer"
unterscheiden, unabhängig von unsichtbaren Zeichen:

```rust
// test_connection.rs, resolve_secret
if let Some(value) = provided { return Ok(SecretString::from(value)); }
```

nimmt jeden `Some`-Wert, auch `""`. Beim Speichern heißt ein nach dem
Trimmen leerer Wert „unverändert lassen" bzw. „Passwort ist
erforderlich". Das Formular schickt bei **Neuanlage** für ein leeres
Pflichtfeld `""` statt `null` (`ServerForm.tsx`, `orNullIfUpdate` greift
nur beim Bearbeiten), und derselbe `buildInput()` bedient den
Verbindungstest. „Neuer Server, Passwortfeld leer, Verbindung testen"
meldet damit auf einem Server mit `PermitEmptyPasswords yes` einen
Erfolg für ein Profil, das sich anschließend nicht speichern lässt.

Ein reiner Trim ohne die Leer-Regel würde diesen Fall **häufiger**
machen, weil ein Leerraum- oder BOM-Paste dann auf `""` fällt. Die Frage
trägt deshalb eine Option, die beides zusammen ändert. Bis sie
entschieden ist, bleibt `resolve_secret` unangetastet — halbherzig
nachzuziehen wäre hier schlechter als gar nicht.
