# Spec 0076 — Anmeldung mit einer Schlüsseldatei

Status: umgesetzt
Zweck: Ein Server kann sich mit einer privaten Schlüsseldatei auf der Platte anmelden, so wie `ssh` es täte; ein Knopf überführt die Anmeldeart jederzeit in einen in der verschlüsselten Datenbank gespeicherten Schlüssel.
Bezüge: Spec 0005 (SSH-Modul), Spec 0008 (Server-Formular), Spec 0024 (stabile Fehlercodes), Spec 0073 (Trim von Zugangsdaten), Spec 0075 (Import erzeugt diese Anmeldeart), Spec 0082 (Bearbeiten räumt Secrets nur nach Erfolg auf), Spec 0098 und 0101 (Secret-Speicher), ADR 0065.
Review-Priorität: ERHÖHT (Credential-Handling, Ausführungspfad, erste Stelle, an der Smart SSH eine Schlüsseldatei von der Platte liest)

## Entscheidungen

- **E-1** Dateirechte: genauso ablehnen wie `ssh` (A-4).
- **E-2** Symbolischen Links wird gefolgt, wie bei `ssh`; die Rechteprüfung
  gilt der Datei, die tatsächlich gelesen wird, nicht dem Link (A-3).
- **E-3** Die Passphrase ist optional und liegt wie bei „Private Key" im
  Secret-Speicher (A-5).
- **E-4** Nach der Überführung wird kein Löschen der Ursprungsdatei
  angeboten; der Nutzer löscht selbst (C-5).
- **E-5** Die Datei wird bei jedem Verbindungsaufbau neu gelesen, nie
  zwischengespeichert (A-3, 4.3).

Nicht Teil dieser Spec: ein Rückweg vom gespeicherten Schlüssel zur Datei
(das hieße, Schlüsselmaterial auf die Platte zu schreiben), das Erzeugen,
Löschen oder Umrechten von Schlüsseldateien, ein Ersatz für den SSH-Agent
(er bleibt eine eigene Anmeldeart) und der Import selbst (Spec 0075).

## 1. Anmeldeart

**A-1** Neben Passwort, Privater Schlüssel, Agent und Zertifikat gibt es die
fünfte Anmeldeart „Schlüsseldatei": ein Pfad und eine optionale Passphrase.
Der Pfad wird gespeichert, **wie der Nutzer ihn angegeben hat** — kein
Auflösen von Links, keine Normalisierung, kein Ersetzen von `~`, kein Trim.
Einzige Ausnahme ist die Leer-Prüfung: Ein Pfad, der nur aus Leerraum oder
unsichtbaren Randzeichen besteht, gilt als nicht angegeben und ergibt
„Pfad zur Schlüsseldatei ist erforderlich" (Code
`SERVER_IDENTITY_FILE_REQUIRED`) — beim Speichern wie im Verbindungstest,
vor jedem Lesen. Der Pfad ist kein Secret; die Datenbankspalte der
Anmeldeart braucht dafür keine Migration.

**A-2** Für die Anmeldung ist ein Schlüssel aus einer Datei **nicht
unterscheidbar** von einem gespeicherten Schlüssel: Der Transport bekommt
dasselbe Material (OpenSSH-Schlüssel plus optionale Passphrase), und die
Anmeldelogik verzweigt nirgends auf die Herkunft.

**A-3** Beim Verbinden wird die Datei gelesen:

- `~` am Anfang (auf Windows auch `~\`) wird zum Home-Verzeichnis. Ein
  relativer Pfad wird abgelehnt („absoluter Pfad nötig"), ebenso `~user`:
  Es wird kein Benutzer nachgeschlagen, ein solcher Pfad wird wie ein
  relativer behandelt.
- Symbolischen Links wird gefolgt (E-2).
- Es werden **nur reguläre Dateien** gelesen. Ein Verzeichnis, Zeichengerät
  (`/dev/zero`, `/dev/stdin`), benanntes Rohr oder Socket ergibt „keine
  reguläre Datei" — ohne zu blockieren und ohne Wartezeit auf einen
  Schreiber. Dateiart, Rechte und Größe werden auf **demselben geöffneten
  Handle** geprüft, mit dem anschließend gelesen wird, nie auf dem Pfad
  davor.
- Die Größe ist auf **1 MiB** begrenzt, geprüft am Handle **und** beim
  Lesen: Es wird nie mehr als 1 MiB in den Speicher gelesen, auch wenn das
  Handle weniger meldet; die Lesegrenze ist die verbindliche.
- Der Inhalt muss ein gültiger OpenSSH-Schlüssel in Textform sein. Kein
  UTF-8 und NUL-Bytes gelten als ungültig.

**A-4 Dateirechte.** Auf Unix wird die Anmeldung **abgelehnt**, sobald die
Datei für Gruppe oder Welt **irgendein** Recht trägt (Rechte-Bits `077`,
also auch ein reines Schreibrecht) — die Regel von OpenSSH, an einer fremden
Datei sogar unbedingt. Die Meldung sagt „für Gruppe oder Welt lesbar", nennt
Pfad und tatsächliche Rechte und den Befehl `chmod 600 <Pfad>`. Auf Windows
entfällt die Prüfung, wie bei OpenSSH.

A-4 gilt für die Anmeldung, **nicht** für den Vorab-Befund (B-3) und nicht
für die Überführung (C-3): Beide lesen eine Datei mit zu weiten Rechten und
**melden** den Mangel. Sonst wäre der Knopf aus C-1 genau dann gesperrt,
wenn er gebraucht wird; er behebt den Mangel ja.

**A-5** Ist der Schlüssel verschlüsselt und keine Passphrase hinterlegt,
sagt die Meldung genau das und nennt den Pfad („… ist verschlüsselt, aber es
ist keine Passphrase hinterlegt") — nicht „Anmeldung fehlgeschlagen". Ein
verschlüsselter Schlüssel **mit** hinterlegter Passphrase verbindet; das
Lesen selbst entscheidet das nie, sondern meldet nur „verschlüsselt".
Die Passphrase wird wie bei „Private Key" behandelt: Spec 0073 (Trim), leer
beim Bearbeiten heißt „unverändert", ein Fehler des Secret-Speichers beim
Lesen ist ein eigener Fehler und keine „fehlende Passphrase".

**A-6 Fehlerfälle, jeder mit eigener Meldung; keine enthält Dateiinhalt:**

| Fall | Meldung nennt |
|---|---|
| Datei fehlt (auch toter Link) | den Pfad, „nicht gefunden" |
| keine Leseberechtigung, Pfad nicht öffenbar | den Pfad, „nicht lesbar" |
| Rechte zu weit (A-4) | den Pfad, die Rechte, den `chmod`-Befehl |
| größer als 1 MiB | den Pfad und die Grenze |
| kein gültiger OpenSSH-Schlüssel (auch kein UTF-8, NUL-Bytes) | den Pfad, „enthält keinen gültigen Schlüssel" |
| verschlüsselt, keine Passphrase hinterlegt (A-5) | den Pfad, „Passphrase hinterlegt" |
| Passphrase falsch | „Passphrase falsch oder Key beschädigt", ohne Pfad |
| relativer Pfad oder `~user` | den Pfad, „absoluter Pfad nötig" |
| kein reguläres Objekt | den Pfad, „keine reguläre Datei" |

Die Fälle bis auf „Passphrase falsch" tragen den Pfad und bleiben bis zur
Anzeige unterscheidbar (stabile Codes `KEY_FILE_NOT_FOUND`,
`KEY_FILE_NOT_READABLE`, `KEY_FILE_PERMISSIONS_TOO_OPEN`,
`KEY_FILE_TOO_LARGE`, `KEY_FILE_NOT_A_REGULAR_FILE`,
`KEY_FILE_PATH_NOT_ABSOLUTE`, `KEY_FILE_INVALID_KEY`; Spec 0024). Die
Oberfläche zeigt je Code einen übersetzten Text (`de` und `en`).

**A-7** Ein Lesefehler ist ein Auflösungsfehler der Anmeldung (Code
`SSH_CREDENTIAL_RESOLUTION_FAILED`), kein eigener Fehlertyp und kein Panic.

**A-8** Die Anmeldeart gilt auch für Jump-Hosts. Die Meldung nennt den
Hop, der gescheitert ist, vorn als `Benutzer@Host:Port: …` — im
Verbindungstest auch für Hops ab dem zweiten.

**A-9** *entfallen.* Ein früher erwogener Punkt (eine unbekannte
Anmeldeart soll nicht die ganze Serverliste mitreißen) gehört nicht zu
dieser Spec; die Kennung wird nicht neu vergeben.

## 2. Oberfläche

**B-1** Beim Anlegen und Bearbeiten eines Servers ist „Schlüsseldatei"
neben „Passwort", „Private Key", „SSH-Agent verwenden" und „Zertifikat"
wählbar.

**B-2** Die Eingabe besteht aus einem Pfadfeld (Platzhalter
`~/.ssh/id_ed25519`, Beschriftung „Pfad zur Schlüsseldatei"), einem
Dateidialog-Knopf „Datei wählen…" und dem optionalen Feld „Passphrase
(optional)". Getippte Pfade bleiben erlaubt.

**B-3** Zum eingetragenen Pfad zeigt das Formular **vor dem Speichern** einen
Befund (kurz nach dem Tippen, solange der Pfad nicht leer ist): ob die Datei
existiert, ob die Rechte passen (A-4), ob sie wie ein OpenSSH-Schlüssel
aussieht und ob sie verschlüsselt ist. Wechselt der Pfad, verschwindet der
alte Befund sofort; bis der neue da ist, steht „Wird geprüft …". Der Inhalt
wird nur dafür benutzt, nirgends gespeichert oder angezeigt. Ein
Fehlbefund — oder ein Befund, der nicht abgefragt werden konnte — hindert das
Speichern **nicht**: Die Datei darf erst später entstehen.

**B-4** Die Serverliste zeigt bei einem Server mit Schlüsseldatei eine Zeile
mit dem Pfad (voller Pfad als Tooltip); die Serverdetails zeigen ihn im
Pfadfeld. Der Pfad ist das einzige Auth-Detail, das hinausgeht — nie
Dateiinhalt, Passphrase oder deren Referenz. Bei jeder anderen Anmeldeart
ist er leer.

**B-5** Alle Texte sind für `de` und `en` übersetzt.

## 3. Überführung in einen gespeicherten Schlüssel

**C-1** Bei einem bereits gespeicherten Server, dessen Anmeldeart
tatsächlich (nicht nur im Entwurf) eine Schlüsseldatei ist, gibt es den
Knopf „In die verschlüsselte Datenbank übernehmen". Er wirkt auf den
gespeicherten Server, nicht auf den offenen Entwurf.

**C-2** Vorher zeigt ein Bestätigungsschritt, **welche Datei** gelesen wird
(voller Pfad), **was sich ändert** (die Anmeldung hängt danach an der
verschlüsselten Datenbank, nicht mehr an der Datei) und **was nicht** (die
Datei bleibt unverändert liegen, wird nicht gelöscht oder verändert).

**C-3** Beim Bestätigen wird die Datei einmal gelesen (ohne die
Rechte-Sperre aus A-4), der Inhalt **byte-gleich** im Secret-Speicher
abgelegt und die Anmeldeart auf „Private Key" umgestellt. Eine vorhandene
Passphrase-Referenz wandert unverändert mit. Ein Server, der keine
Schlüsseldatei benutzt, wird mit `SERVER_NOT_AN_IDENTITY_FILE` abgewiesen,
auch wenn die Oberfläche den Knopf nicht angeboten hätte.

**C-4** Ein verschlüsselter Schlüssel wird **verschlüsselt** übernommen:
Der Dateiinhalt geht byte-gleich (ohne Trim) in den Speicher, entschlüsselt
und gespeichert wird nichts Entschlüsseltes.

**C-5** Die Ursprungsdatei wird nicht angefasst — nicht gelöscht, nicht
geändert, nicht umgerecht (E-4). Auch das Löschen eines Servers entfernt nur
dessen Passphrase, nie die Datei; die Lösch-Vorschau sagt das.

**C-6 Kein halber Zustand, in beide Richtungen.**

- Schlägt das Schreiben in den Secret-Speicher fehl, bleibt der Server
  unverändert auf „Schlüsseldatei". Kann der Speicher vorher nicht
  gelesen werden, wird gar nichts geschrieben.
- Schlägt danach das Speichern des Servers fehl, wird der geschriebene
  Eintrag zurückgenommen; stand dort vorher ein Wert, wird er
  wiederhergestellt, nicht gelöscht.
- Gelingt auch die Rücknahme nicht, sagt die Meldung das (eigener Code
  `IDENTITY_FILE_ROLLBACK_LEFT_KEY_BEHIND`, nennt den liegen gebliebenen
  Eintrag) — der Nutzer erfährt, dass ein Schlüssel ohne Server im Speicher
  liegt.

**C-7** Der Knopf ist erst wählbar, wenn die gespeicherte Datei lesbar und
ein gültiger Schlüssel ist (auch bei zu weiten Rechten, A-4); sonst steht
der Grund daneben (A-6). Konnte der Befund nicht abgefragt werden, bleibt der
Knopf gesperrt und sagt es.

## 4. Zusammenspiel

**4.1** Der Transport kennt keine Anmeldeart „Schlüsseldatei" (A-2); die
Herkunft des Schlüssels ist kein Unterschied im Material.

**4.2 Eigene Grenze zum Dateisystem.** Das Lesen der Datei hat genau eine
Umsetzung und hinter ihr zwei Operationen: **lesen** (gibt den Schlüssel
heraus; ein Schalter entscheidet, ob zu weite Rechte ablehnen, A-4) und
**befunden** (derselbe Weg, gibt nie Schlüsselmaterial heraus; für B-3 und
C-7). Dort entstehen alle pfadtragenden Fehler aus A-6; die Prüfungen
(Pfadform, Dateiart, Rechte, Größe, Gültigkeit) laufen an einer Stelle auf
einem Handle, nicht an zwei. Ob ein verschlüsselter Schlüssel ein Fehler ist,
entscheidet erst die Anmeldeauflösung, die die Passphrase-Referenz kennt
(A-5). Die Grenze ist in Tests durch eine Attrappe ersetzbar.

**4.3** Weil bei jedem Verbinden gelesen wird (E-5), wirkt ein getauschter
Schlüssel sofort, und kein Schlüssel liegt länger als nötig im Speicher.

**4.4** Der Pfad wird unaufgelöst gespeichert (A-1), damit `~/.ssh/…` auf
jedem Rechner auf das dortige Home zeigt.

**4.5** Ein Server mit Schlüsseldatei ist so stark wie die Datei: Eine
zwischen zwei Verbindungen veränderte Datei wird nicht bemerkt. Wer das
nicht will, überführt den Schlüssel (C-1) — auch bei zu weiten Rechten.

## 5. Sicherheitszusagen

**5.1** Schlüsselmaterial bleibt in einem geschützten Secret-Typ ohne
`Debug`-Ausgabe; es erscheint nicht in der Datenbank (außer als
Überführungsergebnis im Secret-Speicher, C-3), nicht im Log, nicht in einer
Debug-Ausgabe des Servers und nicht in der Oberfläche. Eine kurzlebige,
lokale Textzwischenform lässt sich nicht vermeiden (UTF-8-Prüfung); sie wird
auf Fehlerpfaden überschrieben.

**5.2** Keine Meldung enthält Dateiinhalt: Jede nennt Pfad und Grund, nie
eine Zeile aus der Datei. Der Fehlertext der Parse-Bibliothek wird für eine
ungültige Datei nicht durchgereicht, sondern durch eine eigene Meldung
ersetzt.

**5.3** Eine fehlgeschlagene Prüfung aus A-3 oder A-4 **bricht** die
Anmeldung ab: kein Rückfall auf den Agent, kein Weiterprobieren mit leerem
Schlüssel.

**5.4** Gelesen wird genau der eine angegebene Pfad: keine Suche in
Verzeichnissen, keine Vorschläge, kein Probieren von `~/.ssh/id_*`. „Nicht
gefunden" und „nicht lesbar" sind unterscheidbar, weil der Nutzer ohnehin
nur mit seinen eigenen Rechten liest.

**5.5** Die Überführung verändert die Ursprungsdatei nicht (C-5) und speichert
nichts Entschlüsseltes (C-4).

**5.6** Es wird keine bestehende Prüfung abgeschwächt. Filter, Risiko und
Bestätigung verhalten sich bei einem Server mit Schlüsseldatei identisch
zu einem mit gespeichertem Schlüssel; es gibt keine Verzweigung auf die
Anmeldeart in der Sicherheitslogik.

## 6. Prüffälle

**6.1 Auflösung (mit Attrappe statt Dateisystem)**

1. Gültige Datei ergibt dasselbe Material wie ein gespeicherter Schlüssel.
2. Die Passphrase kommt aus dem Secret-Speicher, nicht aus der Datei.
3. Lesefehler ergibt einen Auflösungsfehler, keinen Panic (A-7).
4. Die Anmeldearten ohne Datei ändern ihr Ergebnis nicht.
5. Jeder Fall aus A-6 ist eine eigene unterscheidbare Fehlerart; geprüft
   gegen die Art, nicht gegen den Meldungstext.

**6.2 Dateizugriff**

1. Absoluter Pfad, Rechte `0600`, gültiger Schlüssel: gelesen.
2. `~/…` wird aufgelöst.
3. Relativer Pfad: „absoluter Pfad nötig".
4. Rechte `0644` auf Unix: abgelehnt mit `chmod`-Befehl; auf Windows
   übersprungen.
5. Symbolischer Link auf eine gültige Datei: gefolgt, Rechteprüfung auf dem
   Ziel.
6. Datei über 1 MiB: abgelehnt, nicht vollständig gelesen; eine
   Quelle, die Größe 0 meldet und endlos liefert, wird an der Lesegrenze
   gestoppt.
7. Verzeichnis, Zeichengerät, benanntes Rohr: je „keine reguläre Datei";
   das Rohr blockiert nicht.
8. `~user/…`: „absoluter Pfad nötig", ohne Benutzer aufzulösen.
9. Der Befund meldet dasselbe wie das Lesen, inklusive Problem bei
   relativem Pfad, fehlender, zu großer und Nicht-Datei, und gibt nie
   Schlüsselmaterial heraus.
10. Lesen ohne Rechte-Sperre liefert eine Datei mit `0644`; mit Sperre wird
    sie abgelehnt. Auch Rechte `0620` werden abgelehnt.

**6.3 Oberfläche und Überführung**

1. Server mit Schlüsseldatei anlegen, speichern, neu laden, bearbeiten: die
   Anmeldeart überlebt den Rundlauf durch die Datenbank. Beim Bearbeiten
   bleibt die gespeicherte Passphrase erhalten.
2. Der Befund meldet fehlende Datei, zu weite Rechte und „verschlüsselt"
   richtig und hindert das Speichern nicht.
   2a. Liste und Details zeigen den Pfad; bei jeder anderen Anmeldeart ist
   das Feld leer.
   2b. Eine Datei mit Rechten `0644` lässt sich überführen, und der Knopf
   ist wählbar, obwohl die Anmeldung damit abgelehnt würde.
3. Überführung eines unverschlüsselten Schlüssels: danach „Private Key",
   Verbindung unverändert erfolgreich.
4. Überführung eines passphrase-geschützten Schlüssels: Inhalt byte-gleich,
   Passphrase-Referenz mitgewandert, Verbindung klappt.
5. Schreiben in den Speicher scheitert: Server unverändert; Speichern des
   Servers scheitert: Eintrag wieder weg (oder Vorwert wiederhergestellt),
   und scheitert die Rücknahme, sagt es die Meldung.
6. Ein Jump-Host mit Schlüsseldatei verbindet; scheitert er, nennt die
   Meldung den Hop.
7. Falsche Passphrase: die Meldung sagt „Passphrase falsch …" und enthält
   kein Schlüsselmaterial.
8. Verschlüsselter Schlüssel mit richtiger Passphrase verbindet; das Lesen
   behandelt den Fall nicht selbst als Fehler (A-5).

**6.4 Adversariale Fälle**

1. Kein Schlüsselmaterial außerhalb des geschützten Typs: nach einem
   Verbindungsaufbau steht der Inhalt nicht in der Datenbank (inklusive
   WAL), nicht in einer Debug-Ausgabe des Servers und nicht in der
   Oberfläche (5.1).
2. Keine Meldung verrät Inhalt: leere Datei, Textdatei, Binärdatei mit
   NUL-Bytes, öffentlicher statt privater Schlüssel, abgeschnittener
   Schlüssel ergeben je „kein gültiger Schlüssel" mit Pfad und kein Byte
   aus der Datei (5.2).
3. Kein Rückfall: zu weite Rechte, fehlende Datei, zu große Datei,
   relativer Pfad brechen die Verbindung ab, ohne Agent oder leeren
   Schlüssel zu versuchen (5.3).
4. Die Überführung lässt Prüfsumme und Rechte der Ursprungsdatei
   unverändert (5.5).
5. Nichts Entschlüsseltes im Speicher: der abgelegte Wert beginnt mit
   `-----BEGIN OPENSSH PRIVATE KEY-----` und ist byte-gleich mit der Datei
   (C-4).
6. Pfad-Spielereien: `/dev/stdin`, `/dev/zero`, benanntes Rohr, Link auf
   ein Verzeichnis, Pfad mit NUL-Byte, `..` auf eine Datei außerhalb von
   `~` ergeben einen sauberen Fehler oder einen gelesenen Schlüssel — nie
   einen Hänger, Panic oder unbegrenzten Lesevorgang.
7. Wettlauf: wird die Datei zwischen Rechteprüfung und Lesen gegen eine
   andere getauscht, ist das Ergebnis der alte oder der neue Inhalt, nie ein
   halber, und die Rechteprüfung galt der gelesenen Datei.
8. Keine Verzweigung in der Sicherheitslogik: ein Kommando mit rotem Risiko
   wird auf einem Server mit Schlüsseldatei identisch zu einem mit
   gespeichertem Schlüssel gefiltert, eingestuft und bestätigt (5.6).

## 7. Grenzen

- **Rechte-Prüfung auf Windows** entfällt (wie bei OpenSSH); dort schützt
  nur die Datei-ACL.
- **Netzpfade (UNC) gelten unter Windows als absolut**; ein Schlüssel kann
  so von einer Netzfreigabe gelesen werden. Eine bewusste Entscheidung dazu
  steht aus.
- **Der Wettlauf-Test (6.4.7) ist probabilistisch**; er kann gegen eine
  falsche Umsetzung ausbleiben, gegen die richtige nie falsch anschlagen.
- **`/dev/stdin` (6.4.6)** ist je nach Umgebung Datei, Gerät oder Rohr; der
  Test belegt nur „kein Hänger", die belastbaren Nachweise sind Rohr und
  Lesegrenze.
- **„Nicht im Log" (5.1)** beruht auf Durchsicht, nicht auf einem
  Log-Mitschnitt-Test.
- **Bei falscher Passphrase** hängt die Meldung den Fehlertext der
  Entschlüsselungs-Bibliothek an; er wird nicht durch eine eigene
  Formulierung ersetzt wie bei einer ungültigen Datei (5.2).
- **Eine Datei, die zwischen zwei Verbindungen ausgetauscht wird**, wird
  nicht bemerkt (4.5).
- **Ein fehlgeschlagener Rollback** (C-6) lässt einen Schlüssel ohne Server
  im Speicher zurück; er wird gemeldet, nicht selbst aufgeräumt.
