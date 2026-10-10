# Spec 0077 — Filterregeln mit ungültigem Muster

Status: umgesetzt
Zweck: Ein Glob- oder Regex-Muster, das sich nicht übersetzen lässt, wird beim Speichern abgewiesen. Ist es trotzdem in der Regelliste, wird die Regel bei der Auswertung behandelt, als gäbe es sie nicht, und das geschieht laut.
Bezüge: Spec 0002 (Filter-Engine), Spec 0009 (Regelverwaltung), Spec 0011 (Schnellregel), Spec 0060 (pfadförmige Globs), ADR 0068.

## 1. Ausgangslage

Glob- und Regex-Muster werden erst bei der Auswertung übersetzt. Ein
Übersetzungsfehler ergibt „passt nicht". Für **Allow** ist das gewollt: Eine
kaputt konfigurierte Allow-Regel darf nie versehentlich zu `AutoExec` führen.
Für Deny und Confirm ist dasselbe Verhalten eine Abschwächung: Aus der Regel
wird stillschweigend keine Regel.

Beim Anlegen oder Ändern wurde das Muster früher nirgends geprüft. Das betrifft
das Regel-Formular ebenso wie die Schnellregel aus dem Bestätigungsdialog
(Spec 0011), deren Vorschläge aus Kommando-Wörtern gebaut werden: Enthält ein
Wort `[` oder `{`, ist der vorgeschlagene Glob ungültig (z. B. `ls [abc *`
meldet „unclosed character class").

**Wirkung ohne diese Spec** (Kommando `systemctl stop nginx`):

| Regeln | Ergebnis |
|---|---|
| Allow `systemctl *` und Deny mit gültigem Regex | Deny |
| Allow `systemctl *` und Deny mit Regex, der nicht übersetzt | **AutoExec** |
| nur Deny mit ungültigem Muster | Confirm (wie ohne Regel) |

Eine Deny-Regel, die eine breitere Allow-Regel einschränkt, wäre bei einem
Tippfehler wirkungslos, und das Kommando liefe ohne Bestätigung. Gespeichert
und angezeigt würde die Regel trotzdem, und die Testen-Ansicht zeigte dasselbe
stille „passt nicht".

Auch ein zu großer Regex (z. B. `a{1000}{1000}`: „Compiled regex exceeds size
limit") ist für diese Spec derselbe Fall wie ein Syntaxfehler.

**Pfadförmige Globs haben zwei Zweige** (Spec 0060): den gewöhnlichen und den
strengen mit normalisierten Pfaden. Genau einer von beiden kann scheitern, in
beide Richtungen. Mit einer Allow-Regel `rm *` daneben:

| Deny-Muster | gewöhnlicher Zweig | strenger Zweig | Kommando | Ergebnis |
|---|---|---|---|---|
| `rm /x/[a/../b` | Fehler | ok (`rm /x/b`) | `rm /x/b` | **Deny** (über den strengen Zweig) |
| `rm /x/[a/b]/../c` | ok | Fehler (`rm /x/[a/c`) | `rm /x/a/../c` | **Deny** (über den gewöhnlichen Zweig) |
| `rm /x/[a/b]/../c` | ok | Fehler | `rm /x/c` | AutoExec (kein Zweig passt) |

Der erste Fall entsteht so: Die Normalisierung löscht das Segment vor `..` und
repariert dadurch die offene `[`. Im zweiten Fall bleibt nach dem Löschen eine
`[` ohne Gegenstück stehen. Ein Muster, dessen `..` **innerhalb** einer
Klammer steht (`rm /x/{a/..}/b`), ist kein solcher Fall: Das Segment heißt dort
`..}`, wird nicht als Aufwärts-Segment erkannt, und das Muster übersetzt in
beiden Zweigen.

Eine Regel, die in einem Zweig nicht übersetzt, kann also trotzdem greifen,
über den anderen Zweig. Die Auswertung darf ihr das nicht wegnehmen (3.2.1).

**Entscheidung:** Eine Regel mit ungültigem Muster wird bei der Auswertung
behandelt, **als würde sie nicht existieren**. Es gibt keine Ersatz-Eskalation
auf Confirm oder Deny. Die Lücke schließt die Prüfung beim Speichern
(Schicht 1) für jede neu angelegte oder geänderte Regel. Eine bereits
gespeicherte ungültige Regel bleibt wirkungslos, wird aber sichtbar gemacht
(3.2).

## 2. Ziel und Nicht-Ziele

**Ziel:**

1. Ein Muster, das sich nicht übersetzen lässt, wird gar nicht erst
   gespeichert.
2. Kommt eine Regel doch mit einem solchen Muster in die Auswertung, verhält
   sie sich, als gäbe es sie nicht. Keine Entscheidung wird dadurch milder.
   Das ist **sichtbar**: im Log mit Regel-Kennung und in der Regelliste.

**Nicht-Ziele:**

- Die Auswertung von Risiko-Mustern bleibt unverändert. Deren Muster sind fest
  eingebaut; ein Test stellt sicher, dass sie alle übersetzen (6.2).
- Keine Änderung an Matching-Semantik, Reihenfolge, Geltungsbereich, Priorität
  oder an Spec 0060.
- Muster werden nicht vorab übersetzt oder zwischengespeichert.
- Kein eigenes Größenlimit für Regex: Die Voreinstellung der Bibliothek genügt.
- Keine Ersatz-Eskalation für ungültige Regeln bei der Auswertung.
- Keine Migration, die alte ungültige Regeln löscht oder umschreibt.

## 3. Anforderungen

### 3.1 Schicht 1: beim Anlegen und Ändern abweisen

- **3.1.1** Es gibt eine Prüfung eines Musters, die **genau die Varianten
  übersetzt, die die Auswertung übersetzt**:
  - Regex: das Muster als Regex.
  - Glob: das Muster als Glob. Ist es pfadförmig (Spec 0060, Abschnitt 1),
    zusätzlich der strenge Zweig mit normalisierten Pfaden und ohne `/` für `*`.
  - Exakt ist immer gültig.

  Der Fehler trägt den Fehlertext der Bibliothek.
- **3.1.2** Anlegen und Ändern einer Regel prüfen das Muster, **bevor**
  geschrieben wird. Bei einem Fehler wird nichts gespeichert (Anlegen), und die
  gespeicherte Regel bleibt unverändert (Ändern). Die Schnellregel läuft über
  dieselbe Prüfung. Bei ihr gilt: Die Bestätigung wird trotzdem aufgelöst, und
  der Fehler kommt getrennt zurück.
- **3.1.3** Ein ungültiges Muster erzeugt den Fehlercode
  `FILTER_RULE_PATTERN_INVALID` mit dem Fehlertext der Bibliothek als
  Meldung. Der Code geht auf allen drei Wegen (Anlegen, Ändern, Schnellregel)
  bis zur Oberfläche durch und wird nicht von einer allgemeinen
  Fehlerumwandlung verschluckt.
- **3.1.4** Oberfläche: Das Regel-Formular zeigt für diesen Code den übersetzten
  Satz **und darunter** den Fehlertext der Bibliothek (mit der Stelle des
  Fehlers). Andere Codes im Formular bleiben wie sonst. Die Schnellregel zeigt
  ihren Fehler übersetzt, ohne Fehlertext der Bibliothek: Dort wird ein
  Vorschlag angelegt, kein selbstgeschriebenes Muster.
- **3.1.5** Die Muster-Vorschläge der Schnellregel (Spec 0011) enthalten keinen
  Vorschlag, der die Prüfung nicht besteht. Er wird weggelassen, nicht
  verändert.
- **3.1.6 Wortlaut:** Ein Text für alle Stellen (Formular, Schnellregel,
  Regelliste). Deutsch: „Ungültiges Muster in einer Filterregel. Bitte das
  Muster korrigieren." Die englische Fassung ist sinngemäß gleich.

### 3.2 Schicht 2: bei der Auswertung wie nicht vorhanden, aber laut

- **3.2.1** Eine Regel, deren Muster die Prüfung nicht besteht, verhält sich bei
  der Auswertung so, als gäbe es sie nicht, **soweit ihr Muster nicht
  übersetzt**. Für ein vollständig ungültiges Muster ist das das bisherige
  Verhalten („passt nicht"). An der Entscheidungslogik ändert sich nichts:

  - Es gibt keine neue Entscheidung und keinen neuen Entscheidungs-Code.
  - **Einzelzweig-Fall:** Ein pfadförmiger Glob, bei dem nur **ein** Zweig nicht
    übersetzt, greift weiter über den Zweig, der übersetzt (z. B.
    `rm /x/[a/../b` → Deny). Ihn ganz zu streichen, würde eine greifende
    Deny-Regel abschwächen. „Wie nicht vorhanden" gilt also für das, was nicht
    übersetzt, nicht für den Teil, der wirkt.
- **3.2.1a** Der Freigabe-Abschnitt des KI-Kontexts („Freigegebene Befehle")
  führt Allow-Regeln, deren Muster die Prüfung nicht besteht, nicht auf. Eine
  solche Regel greift nicht, der Befehl braucht weiter eine Bestätigung — die
  KI soll keine Auto-Ausführung erwarten, die nicht stattfindet. Das ändert
  nur den Kontext-Text, nicht die Auswertung.
- **3.2.2** Jede Regel mit ungültigem Muster wird bei der Auswertung auf
  ERROR-Ebene gemeldet, **einmal je Auswertung**, nicht je Teilkommando und
  nicht erst beim ersten Treffer (sonst würden Regeln hinter dem Treffer nie
  gemeldet). Gemeldet werden Regel-Kennung, Aktion und ein fester Kurztext je
  Fall (etwa „regex does not compile", „glob does not compile (strict
  branch)"). **Weder das Muster noch das Kommando noch ein Fehlertext der
  Bibliothek stehen im Log:** Deren Fehlertexte zitieren das Muster wörtlich,
  und Muster wie Kommando können Geheimnisse enthalten. Das gilt für alle drei
  Aktionen.
- **3.2.3** Die Regelliste markiert eine Regel mit ungültigem Muster sichtbar:
  Jede aufgelistete Regel trägt gegebenenfalls den Fehlertext des Musters; die
  Liste zeigt einen Hinweis mit dem Text aus 3.1.6 und dem Fehlertext als
  Detail. Bearbeiten und Löschen bleiben möglich; Speichern verlangt nach
  Schicht 1 ein gültiges Muster. Die Pfeiltasten zum Verschieben der Priorität
  sind an einer solchen Regel deaktiviert (mit Tooltip, der den Grund nennt),
  und bevor eine Nachbarregel geändert wird, bricht das Verschieben ab. So
  bleibt die Regel, dass jeder Schreibweg prüft.
- **3.2.4** Die Auswertung von Mustern für die Risiko-Einstufung bleibt
  unverändert: ein Übersetzungsfehler ist dort weiterhin „passt nicht".

## 4. Verhaltensdetails

- **Warum Schicht 1 die Lücke schließt:** Der Fund entsteht, wenn jemand eine
  Deny-Regel mit Tippfehler speichert. Das verhindert die Prüfung im Formular
  und bei der Schnellregel.
- **Was bleibt (bewusst):** Eine Regel, die vor dieser Spec gespeichert wurde
  oder aus einer künftigen Organisations-Quelle stammt, kann weiter ein
  ungültiges Muster haben. Sie ist dann wirkungslos. Eine Deny-Regel dieser
  Art neben einer breiteren Allow-Regel lässt das Kommando also weiter ohne
  Bestätigung durch. Sichtbar wird das im Log (3.2.2) und in der Regelliste
  (3.2.3). Eine Ersatz-Eskalation (Confirm für den ganzen Geltungsbereich der
  Regel) wurde erwogen und verworfen: Eine kaputte Regel soll nicht mehr
  bewirken als eine fehlende.
- **Verworfen:** den Fehler bei der Auswertung zu „passt" zu machen. Das träfe
  auch Allow-Regeln und die Risiko-Einstufung und machte eine ungültige
  Allow-Regel zu `AutoExec`.

## 5. Sicherheitszusagen

- **Nicht lockern:** Für jede Regelmenge und jedes Kommando ist die
  Entscheidung dieselbe wie ohne diese Spec. Schicht 2 ändert die Auswertung
  nicht, Schicht 1 verhindert nur das Speichern.
- **Allow-Regeln werden nicht weiter:** Im Einzelzweig-Fall greift auch eine
  Allow-Regel über den Zweig, der übersetzt. Das bleibt so.
- **Hard-Blacklist und Risiko-Einstufung bleiben unberührt**, ebenso ihre
  Reihenfolge vor den Nutzerregeln.
- **Neue Datensenke:** das Log aus 3.2.2. Dort stehen nur Regel-Kennung, Aktion
  und ein fester Kurztext, nie das Kommando, nie das Muster.
- **Bekannte, hingenommene Restlücke:** eine bereits gespeicherte ungültige
  Deny-Regel neben einer Allow-Regel (Abschnitt 4).

## 6. Prüffälle

### 6.1 Schicht 1

- **T-1** Anlegen mit Regex `^systemctl stop (.*` → `FILTER_RULE_PATTERN_INVALID`,
  danach ist die Regelliste leer.
- **T-2** Dasselbe mit einem ungültigen Glob (`systemctl [stop`).
- **T-3** Ändern einer gültigen Regel auf ein ungültiges Muster → Fehler; die
  gespeicherte Regel hat danach noch das **alte** Muster.
- **T-4** Beide Einzelzweig-Richtungen aus Abschnitt 1 werden abgewiesen:
  `rm /x/[a/b]/../c` (nur der strenge Zweig scheitert) und `rm /x/[a/../b` (nur
  der gewöhnliche Zweig scheitert). Der Test hält die Vorbedingung (welcher
  Zweig übersetzt) selbst fest.
- **T-5** Gültige Glob-, Regex- und Exakt-Muster werden weiter angenommen
  (u. a. `*`, `**`, Klammern, Anker, Unicode, ein pfadförmiges Muster).
- **T-6** Oberfläche: Die Übersetzung des Codes liefert in `de` und `en` einen
  Text, nicht den Rohcode; die Sprachdateien bleiben paritätisch. Das Formular
  zeigt bei diesem Code den übersetzten Satz **und** den Fehlertext.
- **T-6a** Eine Schnellregel mit einem ungültigen Glob → Fehler, nichts
  gespeichert.
- **T-6b** Vorschläge für `ls [abc def` enthalten keinen Vorschlag, der die
  Prüfung nicht besteht (ohne diese Spec enthielten sie `ls [abc *`).
- **T-6c** Alte Datenbank: Eine Zeile mit ungültigem Regex als Deny wird
  direkt eingefügt. Mit einer Allow-Regel daneben ergibt die echte Auswertung
  dieselbe Entscheidung wie ohne die Regel (`AutoExec`); die Auswertung bricht
  nicht ab, die übrigen Regeln greifen weiter, es gibt ein ERROR-Ereignis mit
  der Regel-Kennung, die Regel wird weiter geladen, und die Auflistung
  markiert sie.
- **T-6d** Die Fehlerumwandlung liefert für ein ungültiges Muster den Code
  `FILTER_RULE_PATTERN_INVALID` und den Fehlertext der Bibliothek als Meldung,
  auf allen drei Wegen (Anlegen, Ändern, Schnellregel).
- **T-6e** Die Auflistung liefert für eine Regel mit ungültigem Muster den
  Fehlertext und für eine gültige keinen; die Liste zeigt bei gesetztem Fehler
  den Hinweis mit Fehlertext. Die Pfeile sind deaktiviert, und es wird auch für
  die Nachbarregel nichts geschrieben.

### 6.2 Fest eingebaute Muster

- **T-7** Alle eingebauten Muster (Hard-Blacklist, Risiko-Muster beider Achsen)
  übersetzen. Der strenge Zweig wird dabei nicht verlangt, weil ihn diese
  Listen nicht benutzen.

### 6.3 Schicht 2, adversarial

Die ungültigen Regeln kommen unter Umgehung von Schicht 1 in die Auswertung
(Fall „Organisations-Quelle"); den Fall „alte Datenbank" deckt T-6c ab.

- **T-A1** Allow `systemctl *` und Deny `^systemctl stop (.*` (ungültig),
  Kommando `systemctl stop nginx` → **AutoExec**, wie mit Allow allein; dazu ein
  ERROR-Ereignis mit der Kennung der Deny-Regel.
- **T-A2** Wie T-A1 mit einem ungültigen Glob. **T-A3** wie T-A1 mit einer
  Confirm-Regel.
- **T-A4** Gültige Deny-Regel **und** ungültige Deny-Regel mit niedrigerer
  Priorität, Kommando passt auf die gültige → **Deny** durch die gültige Regel
  und ein ERROR-Ereignis für die ungültige.
- **T-A5** Allow mit ungültigem Muster allein, Kommando `ls -la` → Confirm
  „keine Regel", nie AutoExec; dazu ein ERROR-Ereignis.
- **T-A6** Tabelle: Aktion {Allow, Confirm, Deny} × Muster {gültig passend,
  gültig nicht passend, ungültig, nur in einem Zweig ungültig} × Typ
  {Glob, Regex}, jeweils neben einer passenden Allow-Regel. Die Entscheidung
  ist in jeder Zeile gleich der des Stands ohne diese Spec.
- **T-A9** Der Regex `a{1000}{1000}` als Deny-Regel → wie T-A1.
- **T-A10** Die Auswertung aus T-A1 mit dem Kommando
  `systemctl stop nginx --password=hunter2` erzeugt ein ERROR-Ereignis mit der
  Regel-Kennung; kein ERROR-Ereignis enthält `hunter2`, und ein Muster mit
  einem Geheimnis erscheint ebenfalls in keinem ERROR-Ereignis.
- **T-A12** Die Einzelzweig-Fälle aus Abschnitt 1, jeweils mit Allow `rm *`
  daneben:
  - Deny `rm /x/[a/../b`, Kommando `rm /x/b` → weiterhin **Deny** mit
    `FILTER_RULE_DENY`, dazu ein ERROR-Ereignis. Scheitert, wenn „wie nicht
    vorhanden" die ganze Regel streicht.
  - Deny `rm /x/[a/b]/../c`, Kommando `rm /x/a/../c` → **Deny** über den
    gewöhnlichen Zweig, dazu ein ERROR-Ereignis.
  - Dieselbe Deny-Regel, Kommando `rm /x/c` → **AutoExec** (kein Zweig passt),
    mit ERROR-Ereignis.
