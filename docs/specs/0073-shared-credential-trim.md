# Spec 0073 — Ein geteilter Trim für Zugangsdaten, inklusive unsichtbarer Zeichen

Status: umgesetzt
Zweck: Zugangsdaten, die ein Nutzer einfügt, werden an den Rändern von Leerraum und von unsichtbaren Kopier-Zeichen (BOM, Zero-Width-Space u. ä.) befreit — überall gleich, im Inneren aber nie verändert.
Bezüge: Spec 0007 (leere Eingabe heißt „unverändert"), Spec 0008 (Server-Formular), Spec 0049 (Anbieter-Fehler sichtbar), ADR 0067.
Review-Priorität: ERHÖHT (Credential-Handling)

## 1. Ausgangslage

Ein eingefügter API-Schlüssel oder ein Passwort mit einem unsichtbaren Zeichen
am Rand — BOM aus einer UTF-8-Datei, Zero-Width-Space aus einer Webseite —
sieht richtig aus, scheitert aber bei der Anmeldung. Das übliche Entfernen von
Leerraum erfasst diese Zeichen nicht, weil sie nicht die Unicode-Eigenschaft
`White_Space` tragen:

| Zeichen | Name |
|---|---|
| U+FEFF | Byte Order Mark |
| U+200B | Zero Width Space |
| U+200C / U+200D | Zero Width Non-Joiner / Joiner |
| U+2060 | Word Joiner |

## 2. Ziel und Nicht-Ziele

**Ziel:** Eine einzige Trim-Semantik für Zugangsdaten, die auch diese
Zeichen am Rand entfernt und von allen Eingabepfaden benutzt wird.

**Nicht-Ziele:**

1. Keine Änderung am Inneren eines Wertes. Ein Secret mit einem
   ungewöhnlichen Zeichen in der Mitte bleibt unangetastet.
2. Keine Normalisierung (kein NFC/NFKC, keine Groß-/Kleinschreibung, kein
   Entfernen von Satzzeichen). Das wäre ein stilles Verändern des Secrets.
3. Kein neues Verhalten bei leerer Eingabe: ein leerer `api_key` heißt weiter
   „Credential unverändert lassen" (Spec 0007, Abschnitt 8.2).

## 3. Anforderungen

**A1** Ein Zugangsdaten-Wert wird an beiden Rändern bereinigt um

- alles, was als Leerraum gilt (bisheriges Verhalten), **und**
- U+FEFF, U+200B, U+200C, U+200D, U+2060.

Die Liste ist bewusst endlich und wird nur nach fachlicher Entscheidung
erweitert.

**A2** Nur an den Rändern. Kein Zeichen innerhalb des verbleibenden Wertes
ändert sich; das Ergebnis ist nie länger als die Eingabe.

**A3** Alle Eingabepfade für Zugangsdaten benutzen dieselbe Semantik:
API-Schlüssel, Basis-URL und Attestierungs-URL eines KI-Anbieters, Passwort,
Passphrase und Sudo-Passwort eines Servers sowie Schlüssel- und
Zertifikatsinhalte. Das Verhalten ist beim Speichern und beim Verbindungstest
(A7) gleich. Anzeigetexte (Namen, Modellnamen) sind davon nicht betroffen.

**A4** Ein Wert, der ausschließlich aus zu entfernenden Zeichen besteht,
ergibt den leeren String. Für den `api_key` heißt das „unverändert lassen",
nicht „mit leer überschreiben". Das ist beabsichtigt.

**A5** Der Vorgang verändert nur den Wert; er schreibt nichts in ein Log und
keine Meldung.

**A6** Ein eingefügter SFTP-Server-Pfad (Spec 0067) wird an den Rändern
ebenfalls so bereinigt. Besteht er nur aus solchen Zeichen, gilt „automatisch
erkennen". Ein unsichtbares Zeichen **im Inneren** führt weiterhin zur
Ablehnung durch die bestehende Plausibilitätsprüfung; ein solches Zeichen
gelangt nie in ein Kommando.

**A7** Der Verbindungstest behandelt alle Zugangsdaten wie das Speichern:
Ist der bereinigte Wert nicht leer, gilt er. Ist er leer oder nicht
angegeben, wird bei einem bestehenden Server das gespeicherte Credential
verwendet, bei einer Neuanlage ergibt das einen Fehler und keinen
Anmeldeversuch mit leerem Wert. Das gilt für Passwort, Schlüsselinhalt,
Zertifikat, Zertifikats-Schlüssel und Passphrase.

**A8** Ein fehlendes Pflichtfeld wird im Verbindungstest mit denselben
Fehlercodes gemeldet wie beim Speichern: `SERVER_PASSWORD_REQUIRED`,
`SERVER_PRIVATE_KEY_REQUIRED`, `SERVER_CERTIFICATE_REQUIRED`,
`SERVER_CERTIFICATE_KEY_REQUIRED`.

## 4. Begründung

- **Eine Stelle statt mehrerer.** Mehrere eigene Fassungen derselben Regel
  driften auseinander; eine Stelle braucht einen Test und eine Ergänzung,
  wenn das nächste Zeichen auffällt.
- **Nur die Ränder.** Ein Secret ist eine undurchsichtige Zeichenkette; am
  Rand ist ein unsichtbares Zeichen praktisch immer ein Kopierunfall, kein
  Anbieter vergibt Schlüssel, die damit beginnen oder enden.
- **Verworfen:** alle nicht druckbaren Zeichen entfernen (zu breit) und
  Unicode-Normalisierung (ändert Secrets still).

## 5. Sicherheitszusagen

- **I1 Kein stilles Verändern.** Nur Randzeichen aus der Liste in A1 fallen
  weg; alles andere bleibt Zeichen für Zeichen erhalten (A2).
- **I2 Kein Secret in Log oder Meldung** (A5).
- **I3 Keine Lockerung.** Es wird mindestens das entfernt, was das bisherige
  Standard-Trimmen entfernt hätte.
- **I4 „Leer" behält seine Bedeutung** (A4): leer heißt „nicht ändern", nicht
  „löschen".

## 6. Testfälle

### 6.1 Die Bereinigung

- T1 Normaler Wert ohne Ränder → unverändert.
- T2 Führende und nachfolgende Leerzeichen, Tabs, `\r`, `\n` → entfernt.
- T3 Je ein Fall für U+FEFF, U+200B, U+200C, U+200D, U+2060 am Anfang, am Ende
  und beidseitig → entfernt.
- T4 Mischung aus Leerraum und unsichtbaren Zeichen am Rand → alle entfernt.
- T5 Dieselben Zeichen in der Mitte → bleiben erhalten (I1).
- T6 Wert nur aus solchen Zeichen → leerer String (A4).
- T7 Leerer String → leerer String.
- T8 Mehrbyte-Zeichen (Emoji, Umlaut) an den Rändern → unverändert, keine
  Panik an einer Zeichengrenze.

### 6.2 Die Eingabepfade

- T9 Ein BOM am `api_key` eines KI-Anbieters wird entfernt.
- T10 Server-Passwort, Passphrase und Sudo-Passwort ebenso.
- T11 Ein `api_key`, der nur aus unsichtbaren Zeichen besteht, führt zu
  „unverändert lassen" (Spec 0049, Fund 1).
- T12 Ein Anzeigename mit Zero-Width-Space bleibt unverändert.

### 6.3 Adversariale Fälle

- X1 100 000 ZWSP am Anfang und ein Nutzzeichen → das Nutzzeichen; die
  Laufzeit bleibt linear.
- X2 100 000 ZWSP ohne Nutzzeichen → leerer String, keine Panik.
- X3 U+2800 (Braille Blank) und U+3164 (Hangul Filler) sind unsichtbar, aber
  nicht in der Liste → bleiben erhalten.
- X4 Ein Secret, das mit `-` oder `_` beginnt → unverändert.
- X5 Für 1 000 zufällige gültige Schlüssel gilt Ausgabe == Eingabe.

### 6.4 Verbindungstest

- T13 Je Slot (Passwort, Schlüssel, Zertifikat, Zertifikats-Schlüssel,
  Passphrase): Randzeichen → dasselbe Secret wie beim Speichern.
- T14 Nur Randzeichen bei einem bestehenden Server → gespeichertes
  Credential; dasselbe bei einer Neuanlage → Fehler mit dem Code aus A8, auch
  bei „bestehender Server mit anderer Anmeldeart, Feld leer".
- T15 SFTP-Pfad: Randzeichen → bereinigt; Zeichen innen → abgelehnt; nur
  unsichtbare Zeichen → „automatisch" (A6).
