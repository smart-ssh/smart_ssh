# Spec 0013 — Sicherheitshärtung, Filter-Engine-Härtung und Kontext-Isolation

Status: umgesetzt
Zweck: Härtungsmaßnahmen aus einem Sicherheitsaudit am Zusammenspiel von KI, Kommando-Filter, Schwärzung und Webview-Laufzeit: Umgehungen des Filters, Einschleusen von Anweisungen über Servermetadaten und -ausgaben, Lücken in der Schwärzung, Webview- und Dateirechte, Jump-Host-Test und Host-Schlüssel. Die Kennungen SEC-01 bis SEC-10 stammen aus dem Audit.
Bezüge: Spec 0002 (Filter-Engine), Spec 0005 (SSH), Spec 0016 (Schwärzung), Spec 0039 (Fencing, Eskalation nach dem Einlesen), Spec 0043 (Ausgabegrenze), Spec 0101 (Datenbank), ADR 0027.

## 1. Ziel

1. **Filter-Umgehungen** durch Zeilenumbrüche, Hintergrund-Operator und
   Prozess-Substitution unterbinden (**SEC-01**).
2. **Einschleusen von Anweisungen** über unbereinigte Servermetadaten
   (Ausgabe von `uname -a`) verhindern (**SEC-02**).
3. **Autonome Schleifen** durch eingeschleuste Anweisungen in
   Server-Ausgaben eingrenzen (**SEC-03**).
4. **Lücken in der Schwärzung** bei Werten mit Leerzeichen oder
   Anführungszeichen und bei modernen Token-Formaten schließen (**SEC-04**).
5. **Webview** durch strikte Inhaltsrichtlinie und eng gefasste
   Dateizugriffe absichern (**SEC-06**).
6. **Jump-Host-Test, Dateirechte, Ausgabegrenze, Host-Schlüssel** korrigieren
   (**SEC-07** bis **SEC-10**).

## 2. Kommando-Parser und Filter-Engine (SEC-01, SEC-05)

Alle Prüfungen dieses Abschnitts (Filter und Risiko-Einstufung) gelten dem
Text, den der Nutzer sieht und freigibt, also vor einer Umwandlung durch eine
Edition (Spec 0038, „Umwandlung ausgehender Texte“). Der umgewandelte Befehl
wird zusätzlich nur gegen die Hard-Blacklist geprüft; ein Treffer lehnt ab,
ohne den umgewandelten Text zu zitieren.

### 2.1 Trenner und Zeilenumbrüche

Außerhalb von Anführungszeichen (`'…'`, `"…"`), Backticks und `$(…)` trennen
Segmente: Zeilenumbrüche (`\n`, `\r`, `\r\n`), `;`, `&&`, `||`, `|` und ein
einzelnes unzitiertes `&` (Hintergrundausführung). Jedes Segment wird
einzeln bewertet; das Gesamturteil ist das strengste.

### 2.2 Steuerzeichen

Ein Kommando mit Nullbyte, Terminal-Escape-Sequenz oder sonstigen
nicht-druckbaren Steuerzeichen (`\x00`–`\x08`, `\x0b`–`\x0c`,
`\x0e`–`\x1f`) ist mehrdeutig („Kommando enthält nicht-druckbare
Steuerzeichen"). Fail-safe: mindestens Bestätigung, nie automatische
Ausführung.

### 2.3 Substitutionen

Neben `$(…)` und Backticks werden auch Prozess-Substitutionen `<(…)` und
`>(…)` erkannt. Ihr Inhalt wird rekursiv geprüft; das Gesamtkommando
verlangt mindestens eine Bestätigung. Für die Verschachtelungstiefe gilt
Spec 0043 Fund B.

### 2.4 Harte Sperrliste

Die Muster der harten Sperrliste sind gegen gängige Shell-Umgehungen
gehärtet:

1. **Pfad-Normalisierung:** `rm` wird unabhängig vom Pfad erkannt
   (`/bin/rm`, `/usr/bin/rm`, `\rm`).
2. **Flag-Permutationen:** getrennte Flags (`-r -f`, `-f -r`,
   `--recursive --force`), Flags nach dem Pfad (`rm / -rf`) sowie
   Root- und Heimatziele (`/`, `/*`, `~`, `.`).
3. **`dd`:** jedes `dd` mit `of=/dev/…`, unabhängig von der Position von
   `if=`.
4. **Herunterfahren und Neustart:** auch `/sbin/shutdown`, `/sbin/reboot`,
   `systemctl reboot|poweroff|halt`, `init 0|6`, `telinit 0|6`.
5. **`/etc/shadow`:** auch Veränderung per `tee`, `cp`, `mv`, `truncate`,
   `sed -i`.

## 3. KI-Kontext-Isolation und Schutz vor eingeschleusten Anweisungen (SEC-02, SEC-03)

### 3.1 Servermetadaten im System-Kontext

Die Ausgabe von `uname -a` wird nie ungeprüft in den privilegierten
System-Kontext der KI übernommen. Sie muss einer strengen Positivliste
genügen (Buchstaben, Ziffern, `.`, `_`, `-`, Leerzeichen, Tabulator, `#`,
`:`; höchstens 256 Zeichen, keine Steuerzeichen oder Zeilenumbrüche).
Verletzt sie die Liste, wird die Betriebssystem-Angabe verworfen.

### 3.2 Kommando-Ausgaben als Daten

Ergebnisse ausgeführter Kommandos erscheinen der KI nie ununterscheidbar von
Nutzereingaben. Sie stehen in eindeutig begrenzten Strukturen mit dem
Hinweis, dass der Inhalt unvertrauenswürdige Rohausgabe des Servers ist und
nie als Systemanweisung zu lesen ist. Die Einzelheiten (Rahmen, Maskierung)
beschreibt Spec 0039.

### 3.3 Eskalation nach dem Einlesen von Serverinhalt

Die ursprüngliche Regel dieses Abschnitts — in automatischen Folgerunden
(ab Runde 2) wird jede Aktion zur Bestätigung — ist durch die
konfigurierbare Eskalation nach dem Einlesen ersetzt (Spec 0039 §5: Strict,
Balanced, Standard; ADR 0027). Die Sicherheitszusage bleibt: Eine vom
Nutzer nicht ausgelöste Folgeaktion nach Serverinhalt läuft nie ohne die
dort beschriebene Eskalation ab.

## 4. Schwärzung (SEC-04)

Die Schwärzung (Spec 0016) erfasst:

1. **Werte mit Anführungszeichen oder Leerzeichen:** Zu einem Schlüsselwort
   (z. B. `password`, `token`, `api_key`, `secret`, `passphrase`) mit `:`
   oder `=` wird der gesamte Wert ersetzt, auch in doppelten oder einfachen
   Anführungszeichen und mit Leerzeichen darin (`password="top secret 123"`).
   Dasselbe gilt für Query-Parameter in URLs (`?Passwort=…`).
   Die Schlüsselwortliste gilt je UI-Sprache; die Schwärzung wendet
   **immer die Listen aller unterstützten Sprachen** an, unabhängig von der
   gewählten Oberflächensprache (die KI antwortet nicht verlässlich in
   dieser Sprache, ein Kommando kann aus anderer Quelle stammen).
   - Englisch: `password`, `passwd`, `token`, `api_key`, `secret`,
     `passphrase`, `credentials`, `private key`, `access key` (das
     Leerzeichen steht für Leerzeichen, `_` oder `-`).
   - Deutsch: `Passwort`, `Kennwort`, `Zugangsdaten`, `Geheimnis`,
     `Schlüssel`, `Zugangsschlüssel`, `Token`, auch mit Umlaut-Umschrift
     (`Schluessel`); Groß-/Kleinschreibung egal. Umlaute gelten in
     vorkomponierter und zerlegter Unicode-Form gleich.
   - Bewusst nicht aufgenommen: `pwd` (`PWD=/pfad` steht in jeder
     Umgebungsausgabe), `pass` und `pw` (zu kurz, zu viele Fehltreffer).
   - Text ohne Wert dahinter bleibt unverändert (`Passwort vergessen?`).
   - Jede unterstützte UI-Sprache braucht eine eigene Liste; ein Test
     schlägt fehl, wenn eine fehlt.
2. **Weitere Token- und Geheimnis-Formate:** Bearer-Token (mindestens 16
   Zeichen), private Schlüssel (PEM-Blöcke aller Art, einschließlich PGP),
   AWS-Schlüssel (`AKIA…`, `ASIA…`) und GitHub-Zugriffstoken
   (`ghp_`, `gho_`, `ghu_`, `ghs_`, `ghr_`).

## 5. Webview und Dateirechte (SEC-06, SEC-08)

### 5.1 Inhaltsrichtlinie

Die Webview läuft mit einer strikten Inhaltsrichtlinie: nur eigene Skripte,
Verbindungen nur zur eigenen Anwendung und zur IPC-Schnittstelle, Bilder
nur eigene oder eingebettete. Die Richtlinie darf nicht auf „aus"
zurückgesetzt werden.

### 5.2 Eng gefasste Dateizugriffe

Das Frontend hat kein allgemeines Leserecht auf das Dateisystem. Eine
Schlüssel- oder Zertifikatsdatei liest das Backend erst, nachdem der Nutzer
sie über den nativen Dateidialog ausgewählt hat; das Frontend übergibt nur
einen Titel, nie einen Pfad (Spec 0008, Spec 0076).

### 5.3 Dateirechte unter Unix

- Der App-Datenordner (und der Ordner der Host-Schlüssel und Protokolle)
  hat Rechte `0700`.
- Die Datenbank, die Datei der bekannten Host-Schlüssel, die Einstellungen
  und Protokolldateien haben Rechte `0600`.
- Gelingt das Setzen nicht, wird eine Warnung protokolliert; die App läuft
  weiter.

## 6. SSH-Transport und Verbindung (SEC-07, SEC-09, SEC-10)

### 6.1 Jump-Host im Verbindungstest

Der Verbindungstest löst Zugangsdaten zweistufig auf: zuerst im flüchtigen
Speicher mit der aktuellen Formular-Eingabe, danach im echten
Credential-Speicher. So findet ein Jump-Host seine im Schlüsselbund bzw.
verschlüsselten Speicher abgelegten Zugangsdaten, das Ziel nutzt die
Formular-Eingabe (Spec 0073).

### 6.2 Ausgabegrenze

Die Ausgabe eines Kommandos ist auf 2 MB je Ausführung begrenzt; darüber
wird abgeschnitten und das Ergebnis als abgeschnitten markiert
(Spec 0043 Fund A).

### 6.3 Host-Schlüssel

Der Speicher bekannter Host-Schlüssel hält je Host (und Port) **einen
Schlüssel je Algorithmus**. Ein Host kann also gleichzeitig z. B. einen
Ed25519- und einen RSA-Schlüssel haben. Eine Abweichung („Mismatch") wird
nur gemeldet, wenn für denselben Algorithmus bereits ein anderer Schlüssel
gespeichert ist; ein Schlüssel eines neuen Algorithmus gilt als unbekannt
und wird zur Entscheidung vorgelegt (Spec 0005).

## 7. Testfälle

| Nr. | Testfall | Erwartetes Ergebnis |
| :--- | :--- | :--- |
| **T1** | `echo safe\nrm -rf /` gegen Erlauben `echo *` | Zwei Segmente; `rm -rf /` trifft Sperrliste oder Vorgabe; Gesamturteil mindestens `Confirm` oder `Deny`. |
| **T2** | `cmd1 & cmd2` | Zerlegung an `&` in zwei Segmente, beide einzeln bewertet. |
| **T3** | `cat <(malicious)` | Substitution erkannt, Inhalt rekursiv geprüft, mindestens `Confirm`. |
| **T4** | `cmd\x00extra` oder `cmd\x1b…` | Mehrdeutig bzw. `Deny` wegen Steuerzeichen. |
| **T5** | `uname -a` liefert eingeschleuste Anweisung | Metadaten werden verworfen, der System-Kontext bleibt frei davon. |
| **T6** | Server-Ausgabe enthält eingeschleuste Anweisung, KI schlägt in Runde 2 eine Aktion vor | Die Aktion wird nach der Eskalation aus Spec 0039 §5 behandelt, nicht still ausgeführt. |
| **T7** | Schwärzung von `password="top secret 123"` | Der ganze Wert, auch mit Leerzeichen, wird zu `[REDACTED]`. |
| **T8** | Verbindungstest mit Server über Jump-Host | Jump-Host-Zugangsdaten werden aus dem echten Speicher aufgelöst, das Ziel aus der Formular-Eingabe. |
| **T9** | Remote-Kommando erzeugt 10 MB Ausgabe | Puffer wird bei 2 MB gekappt, kein Speicherüberlauf. |
| **T10** | Datei-Erstellung unter Linux/macOS | Datenbank und Host-Schlüssel-Datei haben Rechte `0600`. |

## 8. Grenzen

- Die Sperrliste ist ein Netz gegen gängige Umgehungen, kein Beweis der
  Unumgehbarkeit; die weiteren Schichten (Bestätigung, Risiko-Einstufung)
  bleiben nötig.
- Die Schwärzung erfasst bekannte Formate und Schlüsselwörter; ein
  unbekanntes Format ohne Schlüsselwort wird nicht erkannt (Spec 0016).
