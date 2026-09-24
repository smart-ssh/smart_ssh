# Spec 0073 — Ein geteilter Trim für Zugangsdaten, inklusive unsichtbarer Zeichen

Status: freigegeben (Stefan, 2026-09-23) · Backlog: BL-0149, BL-0148 · Gate: —
Repo: **öffentlich** `smart-ssh` — `crates/core/src/profiles/credentials.rs`
(neuer Helfer), `crates/app-shell/src/{dto,server_credentials}.rs`
Review-Priorität: **ERHÖHT** (Credential-Handling; adversariale Fälle in §6.3)

> Ein eingefügter API-Key mit einem unsichtbaren Zeichen am Rand — BOM aus
> einer Textdatei, Zero-Width-Space aus einer Webseite — scheitert bei der
> Authentifizierung, und niemand sieht warum. Der bestehende Trim entfernt
> nur Zeichen mit `White_Space`-Eigenschaft; BOM und ZWSP haben sie nicht.
> Zugleich existiert dieselbe Trim-Semantik an mehreren Stellen in eigener
> Fassung. Diese Spec macht daraus eine Stelle und erweitert sie dort
> einmal.

## Getroffene Entscheidungen (aus den Backlog-Punkten)

- Erst der geteilte Helfer, dann die Erweiterung — **in dieser
  Reihenfolge**. Beide Punkte verweisen aufeinander: BL-0149 nennt den
  Helfer „den natürlichen Ort", BL-0148 zentral zu lösen.
- Der Helfer gehört in `core`, nicht in `app-shell`: `core` trägt die
  fachliche Logik, `app-shell` übersetzt nur (`CLAUDE.md`, Architekturregeln).

---

## 1. Ausgangslage (belegt)

Dieselbe Semantik, drei Fassungen im öffentlichen Repo:

| Stelle | Was |
|---|---|
| `dto.rs:273` `AiProviderConfigInput::trimmed()` | `api_key`, `base_url`, `attestation_url` je `.trim()` |
| `dto.rs:514` | `raw.trim()` auf einem weiteren Eingabewert |
| `server_credentials.rs:65`, `:187`, `:314` | `value.trim()` bzw. `p.trim()` für Passwort, Passphrase, Sudo-Passwort |

`str::trim` entfernt Zeichen mit der Unicode-Eigenschaft `White_Space`.
**Nicht** darunter fallen unter anderem:

| Zeichen | Name | Woher es typischerweise stammt |
|---|---|---|
| U+FEFF | Byte Order Mark | Key aus einer UTF-8-Datei mit BOM kopiert |
| U+200B | Zero Width Space | Kopie aus einer Webseite |
| U+200C/U+200D | Zero Width Non-Joiner / Joiner | dito |
| U+2060 | Word Joiner | dito |

Ein solcher Key wird unverändert gespeichert, die Authentifizierung
scheitert mit 401, und der Nutzer sieht einen Key, der richtig **aussieht**.
Seit Spec 0049 ist der Provider-Fehler wenigstens im Log sichtbar.

## 2. Ziel und Nicht-Ziele

**Ziel:** Eine einzige, getestete Trim-Semantik für Zugangsdaten, die auch
unsichtbare Randzeichen entfernt, benutzt von allen öffentlichen
Eingabepfaden.

**Nicht-Ziele:**

1. **Keine Änderung am Inneren eines Wertes.** Getrimmt wird nur an den
   Rändern. Ein Secret, das in der Mitte ein ungewöhnliches Zeichen trägt,
   bleibt unangetastet — es könnte dort hingehören.
2. **Keine Normalisierung** (kein NFC/NFKC, keine Groß-/Kleinschreibung,
   kein Entfernen von Bindestrichen). Das wäre stilles Verändern eines
   Secrets.
3. **Kein neues Verhalten bei leerer Eingabe.** Die bestehende Bedeutung
   „leerer `api_key` heißt: Credential unverändert lassen" (Spec 0007,
   Abschnitt 8.2) bleibt — siehe aber A4, sie bekommt einen Testfall.
4. Nicht Teil dieser Spec: weitere Aufrufer außerhalb dieses Repos. Sie
   ziehen nach, sobald der Helfer veröffentlicht ist.

## 3. Anforderungen

**A1 (MUSS)** `core` exportiert einen öffentlichen Helfer, der einen
Zugangsdaten-Wert an den Rändern bereinigt. Er entfernt

- alles, was `char::is_whitespace()` erfüllt (unverändertes bisheriges
  Verhalten), **und**
- die unsichtbaren Zeichen ohne `White_Space`-Eigenschaft: U+FEFF,
  U+200B, U+200C, U+200D, U+2060.

Die Liste steht als benannte Konstante mit Kommentar je Zeichen, nicht als
Literal im Rumpf — sie wächst erfahrungsgemäß.

**A2 (MUSS)** Nur an den Rändern. Der Helfer verändert kein Zeichen
innerhalb des verbleibenden Wertes und gibt keinen Wert zurück, der länger
ist als die Eingabe.

**A3 (MUSS)** Alle in §1 genannten Stellen benutzen den Helfer. Kein
`.trim()` auf einem Zugangsdaten-Wert bleibt zurück; ein `.trim()` auf
etwas anderem (Anzeigenamen, Modellnamen) ist davon nicht betroffen und
bleibt.

**A4 (MUSS)** Ein Wert, der **ausschließlich** aus zu trimmenden Zeichen
besteht, ergibt den leeren String. Für `api_key` bedeutet das nach Spec
0007 „unverändert lassen" — das ist beabsichtigt und wird als solches
getestet, damit es niemandem später als Bug erscheint und er die Semantik
umdreht.

**A5 (MUSS)** Der Helfer arbeitet auf `&str` und gibt `String` zurück;
er nimmt **kein** `SecretString` entgegen und loggt nichts. Wer ihn
benutzt, entscheidet selbst, wann ein Wert zum Secret wird.

## 4. Design

### 4.1 Warum ein Helfer und nicht drei Korrekturen

Drei Fassungen derselben Regel driften auseinander — das ist bereits
passiert: Die Lizenzseite entfernt eine BOM, die öffentliche
Credential-Seite nicht. Eine Stelle heißt: ein Test, eine Ergänzung, wenn
das nächste Zeichen auffällt.

### 4.2 Warum nur die Ränder

Ein Secret ist eine undurchsichtige Zeichenkette. Was in seiner Mitte
steht, darf die App nicht beurteilen. Am **Rand** dagegen ist ein
unsichtbares Zeichen praktisch immer ein Kopierunfall — kein Anbieter
vergibt Schlüssel, die damit beginnen oder enden.

### 4.3 Verworfen

- **Alle nicht druckbaren Zeichen entfernen.** Zu breit; träfe auch
  Zeichen, die ein Anbieter legitim verwenden könnte.
- **Unicode-Normalisierung.** Verändert Secrets still und bringt eine
  Abhängigkeit für einen Fall, den es hier nicht gibt.

## 5. Sicherheits-Invarianten

**Berührt: Credential-Handling.**

- **I1 — Kein stilles Verändern.** Nur Randzeichen aus der Liste in A1
  fallen weg. Alles andere bleibt Zeichen für Zeichen erhalten (A2).
- **I2 — Kein Secret in einem Log oder einer Meldung.** Der Helfer loggt
  nicht (A5). Die bestehende Redaction auf den Aufrufpfaden bleibt
  unverändert.
- **I3 — Keine Lockerung einer bestehenden Prüfung.** Der Helfer entfernt
  mindestens das, was `str::trim` entfernt hätte — er kann per
  Konstruktion nicht **weniger** bereinigen als vorher.
- **I4 — „Leer" behält seine Bedeutung.** A4: Ein vollständig
  weggetrimmter Wert ist leer, und leer heißt weiterhin „nicht ändern",
  nicht „löschen".

## 6. Tests

### 6.1 Der Helfer

- T1 Normaler Wert ohne Ränder → unverändert.
- T2 Führende und nachfolgende Leerzeichen, Tabs, `\r`, `\n` → entfernt
  (bisheriges Verhalten bleibt).
- T3 Je ein Fall für U+FEFF, U+200B, U+200C, U+200D, U+2060 am Anfang, am
  Ende und beidseitig → entfernt. **Scheitert am heutigen Stand.**
- T4 Mischung aus Leerzeichen und unsichtbaren Zeichen in beliebiger
  Reihenfolge am Rand → alle entfernt.
- T5 Dieselben Zeichen **in der Mitte** → bleiben erhalten (I1).
- T6 Wert ausschließlich aus solchen Zeichen → leerer String (A4).
- T7 Leerer String → leerer String.
- T8 Wert mit Mehrbyte-Zeichen an den Rändern (z. B. Emoji, Umlaut) →
  unverändert, keine Panik an einer Zeichengrenze.

### 6.2 Die Aufrufpfade

- T9 `AiProviderConfigInput::trimmed()` entfernt eine BOM am `api_key`.
- T10 Server-Passwort, Passphrase und Sudo-Passwort ebenso.
- T11 Ein `api_key`, der nur aus unsichtbaren Zeichen besteht, führt zu
  „unverändert lassen", **nicht** zum Überschreiben mit leer — der
  bestehende Schutz aus Spec 0049, Fund 1, bleibt wirksam.
- T12 Ein Anzeigename mit einem Zero-Width-Space bleibt unverändert: Der
  Helfer gilt für Zugangsdaten, nicht für Anzeigetexte.

### 6.3 Adversariale Fälle

- X1 **Sehr viele Randzeichen.** Ein Wert mit 100 000 ZWSP am Anfang und
  einem einzigen Nutzzeichen: Ergebnis ist das Nutzzeichen, die Laufzeit
  bleibt linear (kein quadratisches Abschneiden in einer Schleife).
- X2 **Nur-Ränder in voller Länge.** 100 000 ZWSP ohne Nutzzeichen →
  leerer String, keine Panik.
- X3 **Zeichen, die wie Rand aussehen, aber keiner sind.** U+2800
  (Braille Blank) und U+3164 (Hangul Filler) sind unsichtbar, aber
  **nicht** in der Liste — sie bleiben erhalten. Der Test hält fest, dass
  die Liste bewusst endlich ist, statt „alles Unsichtbare" zu raten.
- X4 **Ein Secret, das mit einem legitimen Bindestrich oder Unterstrich
  beginnt** (`-abc`, `_abc`) → unverändert. Der Helfer darf nicht anfangen,
  Satzzeichen zu entfernen.
- X5 **Eingabe ist bereits sauber.** Für 1 000 zufällige gültige Keys
  gilt: Helfer-Ausgabe == Eingabe. Belegt, dass die Änderung im Normalfall
  nichts tut.

## 7. Umsetzungsreihenfolge

1. Helfer in `core` mit T1–T8; T3 zuerst rot schreiben.
2. Aufrufpfade in `dto.rs` umstellen, T9/T11/T12.
3. Aufrufpfade in `server_credentials.rs` umstellen, T10.
4. Adversariale Fälle X1–X5.
5. Gate grün (`&&`-verkettet, nie in einer Pipe, `RUST_EXIT=0` **und**
   `FE_EXIT=0`).

## 8. Offene Punkte

Keine. Beide Backlog-Punkte sind in Zielrichtung und Reihenfolge
festgelegt; die Zeichenliste in A1 ist eine fachliche Festlegung, die der
Coder nicht erweitern soll, ohne es zu melden.

## 9. Klarstellungen

*(wird während der Umsetzung nachgetragen: Datum · Frage-ID · Antwort)*

**2026-09-24 · Q-BL-0149-01 · `normalize_sftp_server_path` benutzt den
Helfer.** Stefan: Option 1. §1 listet `dto.rs:514`, A3 verlangt „alle in
§1 genannten Stellen" — damit gilt der Helfer auch dort, obwohl
`sftp_server_path` kein Zugangsdaten-Wert ist. Die Sicherheitsprüfung
`is_plausible_sftp_server_path` bleibt unverändert und läuft weiterhin auf
dem gespeicherten Endwert; ein unsichtbares Zeichen kann in keinem Fall
ins Kommando gelangen. Geändert wird nur, ob ein eingefügter Pfad mit
Randzeichen als Fehler endet oder bereinigt wird. Tests: Randzeichen →
bereinigt; Zeichen **innen** → weiterhin abgelehnt; nur unsichtbare
Zeichen → `None` („automatisch"). Annahme A-1 ist damit aufgelöst.

**2026-09-24 · K1 (Architekt) · BL-0243 · Die Passphrase in `test_connection`
fällt unter A3.** „Verbindung testen" trimmt die Passphrase heute nicht,
„Speichern" schon — dieselbe Eingabe kann verschieden ausgehen. Die
Passphrase ist ein Zugangsdaten-Wert, A3 verlangt für ihn den geteilten
Helfer; die Stelle wurde bei der Umsetzung nicht erfasst. Keine neue
Anforderung, sondern die Vervollständigung von A3. Test: dieselbe
Passphrase mit Rand-Leerraum, BOM und Zero-Width-Space führt in beiden
Wegen zum selben Ergebnis, je Anmeldeart mit Passphrase.

**2026-09-24 · Q-BL-0149-02 · Verbindungstest und Speichern behandeln alle
Zugangsdaten gleich.** Stefan: Option 2b. `resolve_secret` im
Verbindungstest trimmt über den geteilten Helfer und wendet dieselbe
Leer-Regel an wie das Speichern: nicht leer → dieser Wert; leer oder
nicht angegeben → das gespeicherte Credential des bestehenden Servers,
sonst Fehler. Gilt für Passwort, Schlüsselinhalt, Zertifikat und
Zertifikats-Schlüssel. Sichtbare Folge: Ein leeres Pflichtfeld bei
Neuanlage führt im Verbindungstest zur Fehlermeldung statt zu einem
Anmeldeversuch mit leerem Wert. Tests: je Slot Randzeichen → dasselbe
Secret wie beim Speichern; nur Randzeichen bei Update → gespeichertes
Credential; dasselbe bei Neuanlage → Fehler (scheitert am heutigen Stand).


**2026-09-24 · Q-BL-0149-03 · Der Verbindungstest meldet ein fehlendes
Pflichtfeld mit denselben Codes wie das Speichern.** Stefan: Option 1.
`resolve_secret` in `test_connection.rs` bekommt einen Parameter
`code: &'static str`, so wie `write_or_reuse_secret` ihn schon hat. Die
vier Aufrufe übergeben die Codes des Speicherns:
`SERVER_PASSWORD_REQUIRED`, `SERVER_PRIVATE_KEY_REQUIRED`,
`SERVER_CERTIFICATE_REQUIRED` und `SERVER_CERTIFICATE_KEY_REQUIRED`.
Damit ersetzt diese Klarstellung „sonst der vorhandene Fehler" aus
Q-BL-0149-02 für diesen Zweig. Der Zweig
`keychain_aware_credential_error` bleibt unverändert, und das Frontend
braucht keine Änderung. Tests: je Slot `err.code`, auch im Fall
„bestehender Server mit anderer Anmeldeart, Feld leer". Der
Neuanlage-Test prüft den Code statt des Textanfangs (ADR 0067 §4).
