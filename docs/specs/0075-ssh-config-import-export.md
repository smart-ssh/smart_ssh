# Spec 0075 — Server-Profile aus `ssh_config` importieren und dorthin exportieren

Status: umgesetzt
Zweck: Ein Nutzer liest eine `ssh_config` ein, sieht vorab, welche Server-Profile entstehen würden, und kann seine Server wieder als `ssh_config` hinausschreiben. Die Datei enthält keine Geheimnisse; derselbe Leser und Schreiber laufen in beide Richtungen, der Rundlauf prüft beide Seiten zugleich.
Bezüge: Spec 0109 (verschlüsseltes Voll-Backup, die vollständige Sicherung), Spec 0076 (Anmeldeart Schlüsseldatei), Spec 0073 (Randzeichen von Zugangsdaten), Spec 0002 (Filter-Engine, Schlagworte als Geltungsbereich), ADR 0074, ADR 0075.
Review-Priorität: ERHÖHT — der Import erzeugt Server-Profile, gegen die Filterregeln, Risikobewertung und Ausführungspfad laufen, und er öffnet Dateien, deren Pfade aus einer fremden Datei stammen.

## 1. Modell, auf das der Import trifft

- Ein Server hat Name, Adresse, Port, Benutzername, optional einen Jump-Host (einwertig, ein anderer Server), Schlagworte, genau eine Gruppe und eine Anmeldeart: Passwort, Agent, im Schlüsselbund gespeicherter Schlüssel oder Schlüsseldatei (Spec 0076). Gruppen sind verschachtelbar.
- Schlagworte sind derselbe Typ wie der Geltungsbereich „Schlagwort" der Filter-Engine (§5.2a).
- Beim Anlegen von Hand gibt es keine Pflichtfeldprüfung; der Import bringt für den Namen eine mit (§4.3).
- Eine Jump-Host-Schleife fällt außerhalb des Imports erst beim Verbinden auf; der Import prüft vorab (§5.3).

## 2. Ziel und Nicht-Ziele

**Ziel.** Ein Nutzer kann eine `ssh_config` einlesen, vorher sehen, was entstehen würde, und seine Server wieder hinausschreiben. Der Rundlauf ergibt dieselben Profile (Grenzen: §4.4).

**Nicht-Ziele:**

1. Kein Zugriff auf die Interna anderer Werkzeuge; gelesen werden nur Dateien, die der Nutzer selbst erzeugt.
2. Kein CSV-Import und kein verschlüsseltes Eigenformat. Die vollständige, verschlüsselte Sicherung ist eine andere Funktion (Spec 0109); sie ersetzt diesen Import/Export nicht und umgekehrt.
3. Keine neue Anmeldeart; die Schlüsseldatei als Anmeldeart ist Spec 0076.
4. Keine Schlüsseldatei wird ungefragt geöffnet: nur auf Weg (b) (§3.1.9) und nur die, die die Vorschau vorher namentlich genannt hat (§5.1).
5. Kein Geheimnis im Export: nie Passwort, Passphrase oder Schlüsselmaterial, nur Pfade.
6. Keine Verwaltung der echten `~/.ssh/config`; sie wird nie beschrieben (§3.2.5).

## 3. Anforderungen

### 3.1 Teil A — Import

**3.1.1** Der Import liest eine vom Nutzer im Dateidialog gewählte Datei
im OpenSSH-`ssh_config`-Format. Er liest **keine** Datei von sich aus,
auch nicht `~/.ssh/config`. Weitere Dateien öffnet er nur über
`Include` nach §3.1.4.

**3.1.2** Übernommen werden MUSS mindestens:

| Direktive | Ziel | Anmerkung |
|---|---|---|
| `Host <alias>` | Name | **je Alias** ein Profil; `Host web1 web2` ergibt zwei |
| `HostName` | Adresse | fehlt sie, gilt der Alias als Adresse (`ssh`-Verhalten) |
| `Port` | Port | fehlt sie: 22 |
| `User` | Benutzername | fehlt sie: leer |
| `ProxyJump` | Jump-Host | §3.1.6 |
| `IdentityFile` | Anmeldeart | der Nutzer wählt, §3.1.9 |
| `Include` | Untergruppe | §3.1.4 |

**Randzeichen.** Von jedem übernommenen Wert werden am **Rand** dieselben
unsichtbaren Zeichen entfernt, die Spec 0073 für Zugangsdaten festlegt.
Es ist dieselbe Liste wie dort. Nicht weil es
Zugangsdaten wären — 0073 gilt ausdrücklich nur für die —, sondern damit
ein aus einer Webseite kopierter `HostName` nicht an einem unsichtbaren
Zeichen scheitert. Die **Zeichenliste** wird geteilt, nicht der
Geltungsbereich. Zeichen **innerhalb** eines Werts bleiben unangetastet.
Getrimmt wird **nach** dem Auflösen der Anführungszeichen: `User " max"`
ergibt `max`.

**3.1.3 Platzhalterblöcke werden Vorgaben und Schlagworte, keine
Profile.** Ein `Host`-Muster mit `*`, `?` oder `!` beschreibt keinen
einzelnen Server und wird **nicht** als Profil angelegt. Stattdessen:

- Seine Werte gelten als Vorgabe für alle passenden konkreten Blöcke,
  nach der OpenSSH-Regel „**der erste gewinnt**": Der zuerst gelesene
  Wert für eine Direktive bleibt stehen.
- Jeder passende Server erhält das Muster als **Schlagwort** (Schlagwörter),
  wörtlich, also `*.prod.de`. Passt ein Server auf mehrere Muster, erhält
  er mehrere Schlagworte. **Aus einem Platzhalterblock entsteht niemals
  eine Gruppe** — Gruppen kommen ausschließlich aus Dateien (§4.5).
- Das Muster `Host *` allein erzeugt **kein** Schlagwort; es passt auf
  alles und trägt keine Information.

Die Vorschau zeigt bei jedem betroffenen Feld, aus welchem Block der
Wert stammt.

**3.1.4 `Include` wird gefolgt, und jede eingebundene Datei wird eine
Untergruppe.** Entschieden in der Entscheidung zu `Include`.

- **Pfade werden nicht eingeschränkt.** Absolute Pfade, `~` und `..`
  sind zulässig. Ein relativer Pfad wird relativ zum **Verzeichnis der
  einbindenden Datei** aufgelöst — nicht relativ zu `~/.ssh`, wie
  OpenSSH es für die Nutzerkonfiguration tut, weil die gewählte Datei
  eben nicht `~/.ssh/config` sein muss. Diese Abweichung steht in der
  Vorschau.
- **Platzhalter im Pfad** (`Include conf.d/*.conf`) werden aufgelöst;
  jede getroffene Datei zählt einzeln. Ob ein `Include` ein Muster ist,
  entscheidet allein der geschriebene Wert (`*`, `?`, `[`), nicht das
  Verzeichnis der einbindenden Datei und keine Pfadform der Plattform:
  Liegt die einbindende Datei in einem Verzeichnis mit solchen Zeichen im
  Namen, wird ein Wert ohne Platzhalter als einzelne Datei gefolgt.
  Platzhalter sind nur im letzten Teil des Werts zulässig; steht einer
  weiter vorne, wird der `Include` gemeldet, nicht aufgelöst.
- **Tiefe höchstens 3.** Die gewählte Datei ist Tiefe 0; ein `Include`
  darin führt auf Tiefe 1. Ein `Include` auf Tiefe 3 wird nicht mehr
  gefolgt, sondern als nicht übernommen gemeldet (3.1.5).
- **Keine Anzahlgrenze für Dateien** — wohl aber die Gesamtgrenzen aus
  §3.3, die über alle Dateien zusammen gelten (der Entscheidung zu `Include`).
- **Keine Schleifen.** Jede Datei wird je Import **höchstens einmal**
  gelesen; entschieden über den aufgelösten, kanonischen Pfad. Ein
  zweites Vorkommen wird übersprungen und gemeldet — das erschlägt sowohl
  den Selbstbezug als auch die Kette über mehrere Dateien.
- **Jede eingebundene Datei wird eine Untergruppe**, benannt nach dem
  Dateinamen, mit der Gruppe der einbindenden Datei als Elternteil
  (§4.5).
- **Die Vorschau nennt jede gelesene Datei mit vollem Pfad**, in der
  Reihenfolge, in der sie gelesen wurde. Eine Datei, die nicht lesbar
  ist, lässt den Import nicht scheitern: Sie wird übersprungen und
  gemeldet.

**3.1.4a Eine Datei, die keine `ssh_config` ist, wird übersprungen —
nicht Zeile für Zeile gemeldet.** Erkannt an einem der drei Merkmale:
Sie enthält ein NUL-Byte, sie ist kein gültiges UTF-8, oder sie enthält
**keine einzige** erkannte Direktive. Die Meldung lautet dann
`<pfad>: keine ssh_config, übersprungen` — **ohne Inhalt**. Das gilt für
eingebundene Dateien; für die **gewählte** Datei greift stattdessen
§3.1.13 (Abbruch mit Meldung).

Das ist die Stelle, an der der Parser den Anzeige-Kanal aus §5.4
zumacht, statt dass eine Regel ihn später auffangen müsste.

**3.1.5 Nicht übernommene Direktiven werden gemeldet — mit Namen, nicht
mit Inhalt.** Je Block und Direktive, mit **Datei und Zeilennummer**.
Gemeldet wird der **Name** der Direktive, also das erste Wort der Zeile,
**nie ihr Wert und nie die ganze Zeile**. Sieht das erste Wort nicht wie
ein Direktivenname aus (nicht `[A-Za-z][A-Za-z0-9-]{0,31}`), lautet die
Meldung `unlesbare Zeile` mit der Zeilennummer und sonst nichts.

Stillschweigend verschlucken ist ausgeschlossen: der Nutzer erfährt **dass** und **wo** etwas nicht
übernommen wurde, nur eben nicht den Inhalt.

**3.1.6 `ProxyJump`** bildet auf den Jump-Host des Servers ab:

- Ein einzelner Hop, dessen Ziel im selben Import (über alle Dateien
  hinweg) oder bereits im Bestand vorkommt → Jump-Host zeigt darauf.
- **Mehrere Hops** (`ProxyJump a,b,c`): Unser Modell kettet über je ein
  Jump-Host **pro Server**, kann eine Kette also nur darstellen, indem
  es Jump-Host **an den Zwischenstationen** setzt.

  **Die Richtung gehört hingeschrieben, sonst rät sie jemand.** In
  OpenSSH ist `a` der **zuerst** kontaktierte Hop; bei uns zeigt
  Jump-Host auf den Hop, **über den** ein Server erreicht wird. Für
  `Host x` mit `ProxyJump a,b,c` entstehen also genau diese Kanten:

  ```
  x springt über c
  c springt über b
  b springt über a
  a springt über niemanden
  ```

  „Zwischenstation" meint im Folgenden `a`, `b` und `c` — alle in der
  `ProxyJump`-Zeile genannten Hosts, auch `a`, an dem am Ende kein
  Jump-Host steht. Daraus folgen zwei harte Bedingungen, ohne die der
  Import bestehende Profile verändern oder Unmögliches versuchen würde:

  1. **Alle Zwischenstationen entstehen in diesem Import neu.** Liegt
     auch nur eine schon im Bestand, wird die Kette **nicht** angelegt —
     sie zu bauen hieße, ein fremdes Profil zu ändern, und das verbietet
     3.1.8.
  2. **Für das Jump-Host einer Zwischenstation gibt es genau eine
     Quelle.** Der Jump-Host eines Servers ist einwertig. Eine zweite Quelle entsteht auf zwei Wegen, und
     beide zählen: wenn `b` in **zwei Ketten** mit verschiedenen
     Vorgängern vorkommt, **oder** wenn `b` in seinem **eigenen**
     `Host`-Block ein `ProxyJump` trägt (`Host x` mit `ProxyJump a,b`
     und zugleich `Host b` mit `ProxyJump q`). In beiden Fällen gilt
     dieselbe Rechtsfolge.

  Ist eine der beiden Bedingungen verletzt, wird `ProxyJump` bei **allen**
  beteiligten Einträgen als nicht übernommen gemeldet (3.1.5), mit der
  Begründung — nicht gekürzt und nicht teilweise angelegt.
- **Schleifen** (`a` über `b`, `b` über `a`, oder länger, auch über
  mehrere Dateien): Der Import legt sie **nicht** an und meldet
  `ProxyJump` bei **allen** beteiligten Einträgen als nicht übernommen.
  Außerhalb des Imports fällt eine Schleife erst beim Verbinden auf; der Import prüft vorab (§5.3).
- Ein Hop, der sich nicht auflösen lässt (Host weder in einer der
  gelesenen Dateien noch im Bestand, oder `user@host:port`-Form ohne
  zugehöriges Profil) → nicht übernommen, gemeldet.
- `ProxyJump none` → kein Jump-Host.
- Zeigt ein Hop auf den lokalen Pseudo-Server, wird er abgelehnt
  (Fehler „Jump-Host darf nicht der lokale Server sein").

**3.1.7 Vorschau vor dem Anlegen.** Der Nutzer sieht die vollständige
Liste der Profile, die entstehen würden, samt Feldern, Herkunft der
Werte (3.1.3), gelesenen Dateien (3.1.4), nicht übernommenen Direktiven
(3.1.5), entstehender Gruppenstruktur (§4.5) und Konflikten (3.1.8),
**bevor** irgendetwas geschrieben wird. Er kann einzelne Einträge
abwählen. Bricht er ab, entsteht nichts.

**3.1.8 Kein Überschreiben.** Existiert bereits ein Server mit gleichem
Name **oder** gleicher Kombination aus Adresse, Port und Benutzername,
wird nicht überschrieben. Die Vorschau zeigt den Konflikt; der Nutzer
wählt je Eintrag: überspringen (Vorgabe) oder als neues Profil mit
abweichendem Namen anlegen.
Das gilt auch für Server, die als „nicht nutzbar" markiert sind
(Spec 0008, 6a); als `ProxyJump`-Ziel werden sie nie aufgelöst.

**3.1.9 `IdentityFile` — der Nutzer entscheidet, was damit geschieht.**
Die Vorschau bietet drei Wege an, umstellbar für den ganzen Import und
einzeln je Eintrag. Voreingestellt ist (a), weil nur dieser Weg keine
einzige zusätzliche Datei öffnet:

**(a) Als Schlüsseldatei übernehmen** *(Vorgabe)*. Der Server bekommt
Anmeldeart „Schlüsseldatei" (Pfad, ohne Passphrase-Verweis) (Spec 0076).
Der Pfad wird **unverändert** übernommen — kein `realpath`, keine
Existenzprüfung, kein Auflösen von `~` (§4.6). **Die Datei bleibt
ungeöffnet.** Später überführt der Nutzer sie mit einem Knopfdruck in
einen gespeicherten Schlüssel, wenn er will (Spec 0076).

  **Ein relativer Pfad oder ein Pfad in `~user`-Form wird übernommen,
  aber sichtbar markiert.** `ssh_config` erlaubt beides; Spec 0076 (A-3)
  lehnt beides beim **Verbinden** ab. Ohne Hinweis entstünden hier
  stillschweigend Server, die sich nie anmelden können. Die Vorschau
  kennzeichnet solche Einträge deshalb als „beim Verbinden nicht
  benutzbar, absoluter Pfad nötig".

**(b) Schlüssel einlesen und im Schlüsselbund ablegen.** Beim
**Bestätigen** des Imports — nicht in der Vorschau — wird jede betroffene
Datei **einmal** gelesen und ihr Inhalt in den Schlüsselbund gelegt; der
Server bekommt „im Schlüsselbund gespeicherter Schlüssel". Dabei gilt:

  - Die Vorschau nennt **vorher** jede Datei, die dabei geöffnet würde,
    mit vollem Pfad, und wie viele es insgesamt sind.
  - Eine Datei, die fehlt, nicht lesbar ist oder kein gültiger
    OpenSSH-Schlüssel ist, lässt den Import **nicht** scheitern: Dieser
    eine Server fällt auf Weg (a) zurück und die Meldung sagt, warum.
  - **Ein verschlüsselter Schlüssel wird verschlüsselt übernommen**
    (Spec 0076: der Dateiinhalt geht byte-gleich in den
    Schlüsselbund). **Nach einer Passphrase wird beim Import nicht
    gefragt.** Die Vorschau sagt bei solchen Einträgen, dass die
    Passphrase nachzutragen ist, bevor die erste Verbindung gelingt.
  - Der Dateiinhalt geht **ausschließlich** in den Schlüsselbund —
    nicht in den Plan, nicht in die Datenbank, nicht in ein Log
    (§5.1, §6.4.1).

**(c) Nicht übernehmen.** Der Server bekommt Agent,
`IdentityFile` erscheint unter „nicht übernommen" (3.1.5).

Mehrere `IdentityFile`-Zeilen in einem Block: Der erste Wert gewinnt, die
weiteren werden als nicht übernommen gemeldet.

**3.1.10 Wiederholbarkeit.** Ein zweiter Import derselben Datei ohne
zwischenzeitliche Änderung legt **nichts** an (alles Konflikt, alles
Vorgabe „überspringen") — auch keine zweite Gruppe.

**3.1.11 Alles oder nichts je Bestätigung.** Schlägt das Anlegen mitten
in einem bestätigten Import fehl, bleibt kein halb angelegter Zustand:
Bereits erzeugte Profile **und Gruppen** dieses Laufs werden
zurückgenommen, und die Meldung nennt den fehlgeschlagenen Eintrag. Das
entspricht dem Rollback-Verhalten beim Anlegen von Hand.

**Reihenfolge:** Jump-Host-Ziele werden vor den Profilen angelegt, die auf sie zeigen, Elterngruppen vor Untergruppen, Gruppen vor ihren Servern.

**3.1.12 Keine Sicherheitseinstellung aus der Datei.** Der Import setzt
die Nachprüfungs-Richtlinie für Ausgaben und die Injection-Prüfung der KI **ausschließlich**
auf die Vorgabewerte des Produkts. Keine Direktive und kein Kommentar
darf sie beeinflussen (§5.2).

**3.1.13** Der Import MUSS in einer verständlichen Meldung scheitern und
nichts anlegen, wenn: die gewählte Datei nicht lesbar ist, in keiner
gelesenen Datei ein `Host`-Block gefunden wird, oder eine Grenze aus
§3.3 überschritten ist.

### 3.2 Teil B — Export

**3.2.1** Exportiert werden je Server: `Host <name>`, `HostName <host>`,
`Port <port>` (wenn ≠ 22), `User <username>` (wenn nicht leer),
`ProxyJump <Alias des Jump-Hosts>` (wenn gesetzt) und `IdentityFile
<pfad>` — Letzteres genau dann, wenn die Anmeldeart des Servers
„Schlüsseldatei" ist (Spec 0076).

**`ProxyJump` nennt den Alias, nicht den Namen.** Wurde der Jump-Host
beim Export nach §4.3 umbenannt (Leerzeichen im Namen, Kollision), trägt
die `ProxyJump`-Zeile den **umbenannten** Alias — sonst zeigt sie auf
einen `Host`-Block, den es in der Datei nicht gibt, und §3.2.6
scheitert.

**3.2.2 Kein Geheimnis in der Datei.** Nie ein Passwort, nie eine
Passphrase, nie Schlüsselmaterial. Ein Server mit
Passwort verliert beim Export seine Anmeldung — das ist
beabsichtigt und wird als Kommentar vermerkt (3.2.3).

**3.2.3 Was sich nicht abbilden lässt, wird benannt.** Gruppen,
Schlagworte, Notizen, Filterregeln, Risiko-Einstellungen,
Nachprüfungs-Richtlinie, Injection-Prüfung, Startverzeichnis und SFTP-Pfad
und die Art der Anmeldung haben in `ssh_config` kein Gegenstück. Das
trifft ausdrücklich auch den Server, dessen Schlüssel **im
Schlüsselbund** liegt („im Schlüsselbund gespeicherter Schlüssel"): Für ihn gibt es keinen
Pfad, den wir schreiben könnten, also steht im Kommentar, dass sein
Schlüssel in der Datei fehlt und woher er stammt. Diese Angaben
erscheinen als Kommentarzeilen über dem jeweiligen Block, beginnend mit
`# smart-ssh:`, und zusätzlich in einer Zusammenfassung im Kopf der
Datei. Diese Kommentare sind **Hinweis, kein Format**: Der Import liest
sie nicht zurück (§4.4).

**3.2.4** Der lokale Pseudo-Server 
wird **nicht** exportiert; er ist kein SSH-Ziel.

**3.2.5 Keine bestehende Datei wird überschrieben.** Der Export schreibt
in eine vom Nutzer gewählte **neue** Datei. Existiert sie bereits, fragt
der Dateidialog des Systems. **Unabhängig davon lehnt der Export
`~/.ssh/config` selbst als Ziel ab** — auch wenn der Nutzer sie im Dialog
auswählt und bestätigt. Es ist die Datei, deren Verlust am teuersten
wäre, und der Export hat keinen Grund, sie anzurühren (§2,
Nicht-Ziel 7). Nach dem Schreiben nennt die Meldung den Pfad und die
Zeile, mit der man die Datei einbindet: `Include <pfad>` am **Anfang**
von `~/.ssh/config` (OpenSSH: der erste Wert gewinnt).

**3.2.6** Die erzeugte Datei MUSS von `ssh` akzeptiert werden: `ssh -F
<datei> -G <host>` liefert Rückgabewert 0 und die erwarteten Werte
(§6.3.2).

**3.2.7** Namen, die in `ssh_config` nicht als `Host`-Alias taugen
(Leerzeichen, `#`, Anführungszeichen, `*`, `?`, leer), werden für den
Export in einen gültigen Alias überführt. Die Regel steht in §4.3; die
Abbildung erscheint in der Meldung, damit der Nutzer den neuen Namen
kennt.

**3.2.8 Hinweis auf das verschlüsselte Backup.** Der Export-Dialog und die Meldung nach dem Export zeigen den Hinweis: „Enthält keine Zugangsdaten, Regeln oder Notizen – für ein vollständiges Backup den verschlüsselten Export nutzen." (englisch: „Contains no credentials, rules or notes. For a complete backup, use the encrypted backup."). Er ersetzt nicht die Aufstellung nach 3.2.3, sondern steht über ihr. Das vollständige Backup liegt an einer eigenen Stelle unter eigenem Namen („Verschlüsseltes Backup", Spec 0109, §9); beide Funktionen haben verschiedene Dialogtitel.

### 3.3 Grenzen

Eine Datei aus fremder Hand darf Smart SSH nicht lahmlegen. Die Grenzen
gelten **über alle gelesenen Dateien zusammen**, nicht je Datei — sonst
umginge eine Kette von `Include`-Dateien jede einzelne (der Entscheidung zu `Include`). Wird
eine überschritten, bricht der Import mit verständlicher Meldung ab und
legt nichts an.

| Grenze | Wert |
|---|---|
| Gelesene Bytes insgesamt | 8 MiB |
| Zeilen insgesamt | 100 000 |
| `Host`-Blöcke insgesamt | 2 000 |
| Länge eines einzelnen Werts | 4 096 Zeichen |
| `Include`-Tiefe | 3 (§3.1.4) |
| Anzahl eingebundener Dateien | unbegrenzt |

## 4. Abbildung und Darstellung

### 4.1 Lesen und Dateizugriff

Der Parser liest selbst keine Dateien; er liefert Blöcke und rohe `Include`-Werte. Das Öffnen der gewählten und der eingebundenen Dateien geschieht getrennt davon, unter den Regeln aus §3.1.4, §3.1.4a und §3.3. Die Grenzen aus §3.3 greifen, bevor Inhalt in den Parser geht.

### 4.2 Parser

- Erkannt werden genau die Direktiven aus §3.1.2, ohne Rücksicht auf Groß-/Kleinschreibung; Werte bleiben unverändert. Anführungszeichen und `=` als Trenner werden nach OpenSSH-Regeln gelesen, Kommentare und Leerzeilen ändern nichts.
- Ob eine eingebundene Datei eine `ssh_config` ist (§3.1.4a), entscheidet eine feste Liste bekannter Schlüsselwörter (`Host`, `Match`, `Include` gehören dazu), nicht die bloße Form des ersten Worts. Die Liste muss nicht vollständig sein: Eine Lücke führt höchstens dazu, dass eine dünn besetzte echte Datei übersprungen und mit Pfad gemeldet wird — die Richtung, in der nichts nach außen gelangt. Die Form des Direktivennamens (§3.1.5) entscheidet nur, ob innerhalb einer anerkannten Datei ein Name oder „unlesbare Zeile" gemeldet wird.
- `Match` wird nicht ausgewertet, weil es von Laufzeit und Umgebung abhängt. Es ist eine **Blockgrenze**: Es beendet den laufenden `Host`-Block, keine Zeile darin wirkt auf ein Profil. `Match` und jede Direktive darin werden nach §3.1.5 gemeldet (Name, Zeilennummer, kein Wert). Beispiel: `Host a` / `User x`, danach `Match host b` / `User y` ergibt für `a` den Benutzer `x`. Ein `Include` innerhalb eines `Match`-Blocks wird gemeldet, nicht gefolgt.
- Platzhalter nur in der letzten Pfadkomponente eines `Include`; weiter vorne wird gemeldet. Ein ungültiger `Port` fällt auf 22 zurück **und** erscheint unter „nicht übernommen".
- `Host`-Muster kennen genau `*`, `?` und `!` (OpenSSH-Semantik). Eckige oder geschweifte Klammern sind wörtliche Zeichen: `Host web[1` ist ein wörtlicher Name, `Host web[1-9]` trifft nicht `web3`. Der Musterabgleich ist unabhängig von der Muster-Grammatik der Filter-Engine, weil ein importiertes Schlagwort wörtlich zu einem Geltungsbereich der Filter-Engine wird (§5.2a).
- Leser und Schreiber teilen die Zitier- und Trennerregeln. Damit der Rundlauf sie nicht allein belegt, prüft §6.3.2 den Export mit `ssh` selbst, und zwar mit einem Wert, der Anführungszeichen erzwingt.

### 4.3 Namen

Import: Der `Host`-Alias wird der Name. Ist er schon vergeben, greift §3.1.8. Ist er leer — auch erst nach dem Randzeichen-Trim aus §3.1.2 —, wird **kein Profil angelegt**, sondern der Block als nicht übernommen gemeldet (§3.1.5).

Export: Ein Alias darf keine Leerzeichen, kein `#`, keine Anführungszeichen und keine Platzhalter enthalten. Regel: Zeichen außerhalb `[A-Za-z0-9._-]` werden zu `-`, Mehrfach-`-` zusammengezogen, Ränder beschnitten; ist das Ergebnis leer, wird `server-<n>` benutzt; bei Kollision wird `-2`, `-3` … angehängt. Die Abbildung wird gemeldet (§3.2.7).

Werte folgen einer eigenen Regel: Ein Wert, der Leerzeichen, `#` oder `"` enthält, wird nach OpenSSH-Regeln in Anführungszeichen gesetzt und ein enthaltenes `"` maskiert; sonst bestünde die erzeugte Datei §3.2.6 nicht.

### 4.4 Der Import liest die eigenen Kommentare nicht zurück

Gruppen und Schlagworte werden nicht in `# smart-ssh:`-Kommentare geschrieben und zurückgelesen: Das wäre ein verstecktes Eigenformat in einer Datei, die wie eine `ssh_config` aussieht. Folge: Der Rundlauf (§6.3.3) erhält Name, Adresse, Port, Benutzer, Jump-Host-Kette und Schlüsselpfad — **nicht** Gruppe, Schlagworte und Notizen. Die Meldung nach dem Export sagt das (§3.2.3).

### 4.5 Gruppen kommen nur aus Dateien, Platzhalter immer aus Schlagworten

Ein Server liegt in genau einer Gruppe. Jede Zeile stammt aus genau einer Datei, daher bestimmt die **Herkunftsdatei** die Gruppe; Platzhalterblöcke werden ausnahmslos Schlagworte (§3.1.3), weil sich Muster überlappen und der Import sonst raten müsste:

```
<Dateiname der gewählten Datei>        ← Gruppe des Laufs
├── team.conf                          ← aus Include, Tiefe 1
│   └── kunden.conf                    ← aus Include, Tiefe 2
└── prod.conf
```

Jeder Server liegt in der Gruppe seiner Herkunftsdatei; die Verschachtelung folgt der `Include`-Kette.

Existiert der Gruppenname schon, wird eine **neue** Gruppe mit angehängter Nummer angelegt statt in die bestehende zu schreiben, damit sich zwei Importe nicht unbemerkt vermischen. Die Wiederholbarkeit (§3.1.10) bleibt gewahrt, weil bei einem zweiten Import derselben Datei kein Server übrig bleibt und die Gruppe nicht angelegt wird.

### 4.6 Der Schlüsselpfad steht in der Anmeldeart

Der Pfad einer Schlüsseldatei steht als Zeichenkette in der Anmeldeart, **unverändert**: kein `realpath`, keine Existenzprüfung, kein Auflösen von `~` (`ssh` löst es selbst auf). Es gibt kein zweites Feld daneben, das ihn doppelt führte. Was beim Verbinden mit dem Pfad geschieht, regelt Spec 0076. Nach dem Import auf Weg (a) hat der Nutzer einen Server, der sich wie mit `ssh` per Schlüsseldatei anmeldet, und den Knopf aus Spec 0076, der daraus einen im Schlüsselbund gespeicherten Schlüssel macht.

## 5. Sicherheitszusagen

Berührt sind **Credential-Handling**, mittelbar über die erzeugten
Profile **Ausführungspfad** und **Filter-/Risiko-Bewertung**, und neu:
**Dateizugriff auf Pfade, die aus einer fremden Datei stammen**. Daher Review-Priorität ERHÖHT.

**5.1 Eine Schlüsseldatei wird nur geöffnet, wenn der Nutzer es
ausdrücklich verlangt hat — und nur die angekündigten.**

- **Von sich aus** öffnet der Import ausschließlich `ssh_config`-Dateien:
  die gewählte und die über `Include` erreichten.
- Eine Datei, auf die `IdentityFile` zeigt, wird **nur** auf Weg (b)
  aus §3.1.9 geöffnet, **nur beim Bestätigen** (nie in der Vorschau) und
  **nur**, wenn die Vorschau sie vorher mit vollem Pfad genannt hat. Eine
  Datei, die dort nicht stand, wird nicht geöffnet — auch dann nicht,
  wenn sich die Konfigurationsdatei zwischenzeitlich geändert hat.
- Auf Weg (b) geht der gelesene Inhalt **ausschließlich** in den
  Schlüsselbund: nicht in den Importplan, nicht in die Datenbank, nicht
  in ein Log, nicht in eine Fehlermeldung.
- Der Export schreibt keinen Wert, der aus dem Schlüsselbund stammt.

Nachgewiesen durch §6.4.1, §6.4.1a, §6.4.1b und §6.4.2.

**5.2 Eine fremde Datei ändert keine Sicherheitseinstellung.**
Nachprüfungs-Richtlinie und Injection-Prüfung kommen
ausschließlich aus den Produktvorgaben (3.1.12). Eine `ssh_config` darf
nicht dazu führen, dass ein Profil mit schwächerer Prüfung entsteht.
Nachgewiesen durch §6.4.3.

**5.2a Ein importiertes Schlagwort kann eine bestehende Filterregel
treffen.** Das ist der **einzige** Weg, auf dem eine fremde Datei die
Filterauswertung erreicht, und er gehört benannt: `Server.tags` ist
bewusst derselbe Typ wie `Scope::Tag` der Filter-Engine
. Ein Muster aus der fremden
Datei wird also wörtlich zu einem Geltungsbereich, gegen den bestehende
Regeln greifen.

Entschärfend, und nachgesehen: Die Engine sortiert
`Deny`/`Confirm`/`Allow` als Stufen **vor** der Scope-Genauigkeit
 — ein `Deny` lässt sich durch
ein importiertes Schlagwort **nicht** abschwächen. Was bleibt, ist eine
Tag-`Allow`-Regel, die ein importiertes Profil von der Vorgabe
`Confirm` auf `Allow` hebt. Importierte Schlagworte enthalten meist
`*`, `?` oder `!` (3.1.3), aber **nicht immer**: Ein gemischter Block wie
`Host prod *` gibt dem Server `prod` das buchstäbliche Schlagwort `prod`
. Die Trefferfläche ist also nicht leer.

**Anforderung daraus:** Die Vorschau MUSS Schlagworte, die auf eine
**bestehende** Filterregel passen, als solche kennzeichnen und die
betroffene Regel nennen. Der Nutzer kann sie einzeln abwählen.
Nachgewiesen durch §6.4.3a.

**Vorgabe in der Vorschau:** Jedes importierte Schlagwort ist
standardmäßig **angewählt** — gleich, ob es buchstäblich oder Muster ist
und ob es eine bestehende Tag-`Allow`-, `Confirm`-, `Deny`-Regel oder keine
trifft. Trifft es eine Regel, zeigt die Vorschau neben dem Schlagwort deren
Wirkung an; es gibt keine automatische Abwahl und keine entsprechende
Markierung. Der Nutzer kann jedes Schlagwort umwählen. Bestätigt der Import
einen Eintrag, ohne dass eine Wahl dazu vorliegt, gilt dieselbe Vorgabe:
Alle Schlagworte entstehen.

**5.3 Kein unbemerkter Jump-Host.** Jeder Jump-Host, der beim Import
entsteht, steht in der Vorschau. Eine Jump-Host-**Schleife** wird beim
Import abgelehnt, nicht erst beim Verbinden — sonst fiele sie erst beim Verbinden auf. Nachgewiesen
durch §6.4.4.

**5.4 Aus einer gelesenen Datei gelangt kein Inhalt nach draußen — auch
nicht auf den Bildschirm.**

Weil `Include` gefolgt wird, bestimmt eine fremde Datei, welche weiteren
Dateien wir öffnen, ohne Einschränkung der Pfade. Das ist weniger
dramatisch, als es zunächst klingt, und die Einordnung gehört in die
Spec, damit sie nicht bei jeder Lesung neu erfunden wird:

- **Keine Rechteausweitung.** Wir lesen mit den Rechten des Nutzers und
  können nichts öffnen, was er nicht ohnehin selbst lesen kann.
- **Keine Ausführung.** `ssh_config` ist Beschreibung, kein Programm. Es
  gibt keinen Weg von einer Direktive zu einem Kommando.
- **Was bleibt, ist die Anzeige.** Eine untergeschobene Konfiguration mit
  `Include ~/.ssh/id_rsa` könnte fremden Inhalt in die Vorschau spülen,
  wo er auf einem geteilten Bildschirm, in einem Screenshot oder in einem
  Support-Anhang landet.

**Diesen einen Kanal macht der Parser zu, nicht eine Liste von Verboten**
:

1. Eine eingebundene Datei ohne erkannte Direktive, mit NUL-Byte oder
   ohne gültiges UTF-8 wird übersprungen (3.1.4a) — nicht Zeile für
   Zeile gemeldet.
2. Von einer nicht übernommenen Zeile wird nur der Direktivenname
   gemeldet, nie ihr Wert (3.1.5).
3. Übernommen werden ausschließlich die Werte aus §3.1.2 — und die
   stehen ohnehin als Serverfelder in der Vorschau, dafür ist sie da.

Ergänzend, weil es nichts kostet: Dateiliste und Direktivenliste gehen in
**keinen** KI-Prompt, in keine Telemetrie und in kein Log außerhalb des
Geräts. Und die Vorschau nennt jede gelesene Datei mit vollem Pfad
(3.1.4) — wer eine untergeschobene Datei importiert, sieht vor dem
Anlegen, was sie aufgemacht hat.

Nachgewiesen durch §6.4.9.

**5.5 Auf Weg (a) wird kein Pfad zu einer Schlüsseldatei aufgelöst.** Ein
`IdentityFile`-Wert ist dann für uns eine Zeichenkette (§4.6): kein
`realpath`, keine Existenzprüfung, kein Lesen. Damit gibt es weder
Pfaddurchquerung noch ein Orakel darüber, welche Schlüssel auf dem
Rechner liegen. Auf Weg (b) wird die Datei geöffnet — dann gelten die
Prüfungen aus Spec 0076 (Dateirechte, symbolische Links, Gültigkeit des
Schlüssels), und zwar dieselben wie beim Anlegen eines Servers von Hand.
Der Import erfindet dafür keine eigenen, schwächeren Regeln.

**5.6 Begrenzte Aufnahme.** §3.3, über alle Dateien zusammen, plus
Tiefenbegrenzung und Schleifenerkennung (3.1.4). Eine überlange, tief
verschachtelte oder zyklische Konfiguration führt zu einer Meldung,
nicht zu Speicherdruck oder einer Endlosschleife. Nachgewiesen durch
§6.4.5.

**5.7 Der lokale Pseudo-Server bleibt außen vor** — nicht exportiert
(3.2.4), nicht als Jump-Host importierbar (3.1.6). Keine
Sonderbehandlung in der Sicherheitslogik, nur an der Grenze: genau so,
wie es die Architekturregel verlangt.

## 6. Abnahmefälle

### 6.1 Abbildung (ohne Dateisystem)

1. Ein Block mit `HostName`, `Port`, `User` → ein Plan-Eintrag mit genau
   diesen Werten. **Scheitert bei kaputter Implementierung**, weil ein
   vertauschtes oder verschlucktes Feld sofort abweicht.
2. Block ohne `HostName` → `host` ist der Alias.
3. Block ohne `Port` → 22.
4. Groß-/Kleinschreibung der Direktiven (`hostname`, `HOSTNAME`) wird
   erkannt; Werte bleiben unverändert.
5. Anführungszeichen und `=` als Trenner (`Port=2222`,
   `User "max mustermann"`) werden nach OpenSSH-Regeln gelesen.
6. `Host web1 web2` (mehrere Aliase) → zwei Plan-Einträge mit denselben
   Werten.
7. Platzhalterblock `Host *.prod.de` mit `User deploy`, danach `Host
   web1.prod.de` ohne `User` → `web1.prod.de` bekommt `deploy` **und**
   das Schlagwort `*.prod.de`, und es entsteht **kein** Profil namens
   `*.prod.de` (3.1.3).
8. Derselbe Fall, aber `Host web1.prod.de` setzt `User root` **vor** dem
   Platzhalterblock → `root` bleibt stehen („der erste gewinnt").
9. Ein Host passt auf `*.prod.de` und auf `*.de` → **zwei** Schlagworte.
10. `Host *` erzeugt kein Schlagwort (3.1.3).
11. Zwei `IdentityFile`-Zeilen in einem Block → der erste Wert steht in
    der Anmeldeart, der zweite in der Liste der nicht übernommenen
    Direktiven (3.1.9).
12. Kommentare und Leerzeilen ändern nichts.

### 6.2 Dateien, `Include`, Gruppen 

1. Datei ohne `Include` → eine Gruppe, benannt nach der Datei, alle
   Server darin.
2. Datei bindet `team.conf` ein, das `kunden.conf` einbindet → drei
   Gruppen in der erwarteten Eltern-Kind-Kette, jeder Server in der
   Gruppe seiner Herkunftsdatei (§4.5).
3. `Include conf.d/*.conf` mit drei Treffern → drei Untergruppen.
4. Relativer `Include`-Pfad wird relativ zum Verzeichnis der
   **einbindenden** Datei aufgelöst, nicht zum Arbeitsverzeichnis
   (3.1.4).
5. Absoluter Pfad und ein Pfad mit `..` werden gefolgt (§3.1.4).
6. `Include` auf Tiefe 3 wird nicht mehr gefolgt und erscheint als nicht
   übernommen (3.1.4).
7. Eine nicht lesbare `Include`-Datei lässt den Import weiterlaufen und
   wird gemeldet (3.1.4).
8. Die Vorschau listet jede gelesene Datei mit vollem Pfad, in
   Lesereihenfolge.
9. `ProxyJump` auf einen Host, der in einer **anderen** eingebundenen
   Datei steht, wird aufgelöst (3.1.6).

### 6.3 Vorschau, Konflikte, Export, Rundlauf

1. Zwölf Hosts, Platzhalterblock, ein `ProxyJump` → Vorschau und
   Ergebnis stimmen Feld für Feld überein .
2. **`ssh -F <datei> -G <host>`** gibt 0 zurück und meldet `hostname`,
   `port`, `user` und — für einen Server mit „Schlüsseldatei"
   — `identityfile` wie erwartet (3.2.6). **Auf den konkreten Pfad
   prüfen, nicht auf das Vorkommen des Schlüsselworts:** `ssh -G` gibt
   `identityfile`-Zeilen auch aus seinen Vorgaben aus, ein Test auf das
   bloße Schlüsselwort würde also auch grün, wenn der Export gar nichts
   geschrieben hat. `-G` statt einer echten
   Verbindung: prüft die Datei, braucht kein Netz.
3. **Rundlauf** (Import auf Weg (a)): Export → Import in einen leeren
   Bestand → Name, Adresse, Port, Benutzer, Jump-Host-Kette **und der
   Schlüsselpfad in der Anmeldeart** stimmen mit dem Original überein
   . Gruppe, Schlagworte und Notizen werden
   ausdrücklich **nicht** verglichen (§4.4).
3a. **Rundlauf nach Weg (b):** Ein Server, dessen Schlüssel im
   Schlüsselbund liegt, erzeugt beim Export **kein** `IdentityFile`,
   sondern den Kommentar aus 3.2.3 — und die Datei enthält kein
   Schlüsselmaterial (§6.4.2).
4. Zweiter Import derselben Datei → **null** neue Profile und **keine**
   neue Gruppe (3.1.10).
5. Namenskonflikt → Vorgabe „überspringen"; das bestehende Profil ist
   danach unverändert (Feldvergleich vorher/nachher).
6. Unbekannte Direktive (`Compression yes`) erscheint in der Liste der
   nicht übernommenen Direktiven **mit Datei und Zeilennummer** (3.1.5).
7. `Match`-Block erscheint als nicht übernommen (§4.2).
8. Abbruch in der Vorschau → Serverliste und Gruppenliste unverändert.
9. Fehler mitten im Anlegen → kein Profil **und keine Gruppe** dieses
   Laufs bleibt übrig (3.1.11).
10. Der lokale Pseudo-Server kommt in der exportierten Datei nicht vor
    (3.2.4).
11. Ein Server mit Leerzeichen und `#` im Namen erzeugt einen gültigen
    Alias; die Meldung nennt die Umbenennung (3.2.7).
12. Zwei Server, deren Namen auf denselben Alias abbilden, erhalten
    unterschiedliche Aliase (4.3) — **und** ein Server, dessen Jump-Host
    dabei umbenannt wurde, trägt in `ProxyJump` den umbenannten Alias;
    `ssh -F … -G` besteht (3.2.1).
13. Ein Benutzername mit Leerzeichen wird in Anführungszeichen exportiert;
    `ssh -F … -G` liefert 0 und meldet den Benutzernamen vollständig
    (4.3).
14. `~/.ssh/config` als Exportziel wird abgelehnt, auch bei bestätigtem
    Dialog; die vorhandene Datei ist danach byte-gleich (3.2.5).
15. Ein Eintrag mit **relativem** `IdentityFile`-Pfad und einer mit
    `~user/…` erscheinen in der Vorschau als „beim Verbinden nicht
    benutzbar" und werden trotzdem angelegt (3.1.9 a).
16. Ein **verschlüsselter** Schlüssel auf Weg (b): Der Eintrag ist in
    der Vorschau als „Passphrase nachzutragen" gekennzeichnet; nach dem
    Import steht der Schlüssel verschlüsselt im Schlüsselbund und es
    gibt kein Passphrase-Verweis (3.1.9 b).

### 6.4 Adversariale Fälle (Pflicht bei ERHÖHT)

1. **Weg (a) liest keine Schlüsseldatei.** Eine Konfiguration zeigt mit
   `IdentityFile` auf eine im Test angelegte Datei mit erkennbarem
   Inhalt. Nach einem Import in der Vorgabeeinstellung: Der **Pfad**
   steht in der Anmeldeart, der **Inhalt** kommt nirgends vor — nicht im
   Plan, nicht im Profil, nicht in der Datenbank, nicht im Log. Zusatz:
   Die Datei wird im Test so präpariert, dass jedes Öffnen auffällt
   (nicht lesbare Rechte), und der Import läuft trotzdem durch. Der Test
   scheitert, sobald jemand „hilfsbereit" den Schlüssel einliest.

1a. **Weg (b) liest genau die angekündigten Dateien und sonst keine.**
   Drei Server mit `IdentityFile`, davon einer abgewählt. Nach dem
   bestätigten Import: Die Schlüssel der zwei gewählten liegen im
   Schlüsselbund, der dritte nicht; **der Inhalt keiner** der drei taucht
   im Plan, in der Datenbank oder im Log auf (5.1). Weitere Fälle: eine
   fehlende Datei, eine nicht lesbare Datei und eine Datei, die kein
   gültiger Schlüssel ist → der jeweilige Server fällt auf Weg (a)
   zurück, die anderen werden normal angelegt, und die Meldung nennt den
   Grund (3.1.9).

1b. **Die Vorschau öffnet nichts.** Derselbe Aufbau wie 1a, aber der
   Nutzer bricht in der Vorschau ab. Keine der Schlüsseldateien wurde
   geöffnet (nachgewiesen über Rechte oder Zugriffszeit), und im
   Schlüsselbund liegt nichts (5.1).
2. **Kein Geheimnis im Export.** Ein Server mit Passwort und ein Server
   mit hinterlegtem Schlüssel werden exportiert; die erzeugte Datei wird
   gegen die Leak-Muster und gegen die im Test gesetzten Werte geprüft —
   kein Treffer (3.2.2).
3. **Eine fremde Datei schwächt keine Einstellung.** Eine Konfiguration
   mit `PostIngestPolicy allow`, `AiInjectionCheck no` und denselben
   Werten als `# smart-ssh:`-Kommentar. Ergebnis: beide Felder stehen
   auf den Produktvorgaben (5.2). 
3a. **Ein importiertes Schlagwort trifft eine bestehende Filterregel.**
   Im Bestand liegt eine Tag-Regel mit `Allow` auf `*.prod.de`; die
   importierte Datei enthält einen Platzhalterblock `Host *.prod.de`.
   Erwartung: Die Vorschau kennzeichnet das Schlagwort und nennt die
   Regel (5.2a); der Nutzer kann es abwählen; wählt er ab, trägt das
   Profil das Schlagwort nicht. Zweiter Fall: eine `Deny`-Regel — sie
   bleibt in jedem Fall wirksam (Stufen vor Scope-Genauigkeit). Vorgabe (§5.2a): Das Schlagwort startet in
   beiden Fällen angewählt, die Vorschau zeigt die Wirkung der Regel
   (`Allow`/`Deny`) daneben, keine Abwahl-Markierung; für ein
   buchstäbliches Schlagwort ebenso.

4. **Jump-Host-Schleife.** `a` springt über `b`, `b` über `a`. Der
   Import legt die Schleife **nicht** an und sagt warum (5.3). Zweiter
   Fall: eine Kette über drei Hosts zurück auf den ersten. Dritter Fall:
   die Schleife verteilt über zwei `Include`-Dateien. In allen dreien
   wird `ProxyJump` bei **allen** beteiligten Einträgen als nicht
   übernommen gemeldet, und die Profile selbst entstehen normal (3.1.6).

4a. **Mehr-Hop-Ketten: richtige Richtung, keine fremden Profile.** (a)
   `Host x` mit `ProxyJump a,b,c`, alle neu im Import → es entstehen
   **genau** die Kanten `x→c`, `c→b`, `b→a`, und `a` hat kein
   Jump-Host (3.1.6). Der Test prüft jede Kante einzeln; ohne das
   würde eine seitenverkehrt gebaute Kette grün.
   (b) `b` liegt bereits im Bestand → Kette wird **nicht** angelegt,
   gemeldet, und `b` ist danach Feld für Feld unverändert. (c) `b` kommt
   in zwei Ketten mit verschiedenen Vorgängern vor → beide gemeldet,
   keine angelegt. (d) `b` ist Zwischenstation **und** trägt im eigenen
   Block ein `ProxyJump` → zweite Quelle, dieselbe Rechtsfolge (3.1.6).
5. **Aufblähung und Schleifen in `Include`.**
   (a) Gesamtgröße über 8 MiB, verteilt auf viele kleine Dateien —
   greift die **Gesamt**grenze, nicht die je Datei (§3.3);
   (b) 3 000 `Host`-Blöcke über mehrere Dateien;
   (c) ein Wert mit 100 000 Zeichen;
   (d) eine Datei, die sich **selbst** einbindet;
   (e) `a.conf` → `b.conf` → `a.conf`;
   (f) `Include *.conf` in einem Verzeichnis, das die einbindende Datei
   selbst enthält.
   Jeder Fall: verständliche Meldung, nichts angelegt, kein Hänger
   (5.6).
6. **Bösartige Aliase, Importseite.** `Host ../../etc/passwd`,
   `Host $(whoami)`, `Host a b` mit Steuerzeichen, ein Alias mit `\n` im
   Wert. Ergebnis: entweder abgelehnt oder als harmloser Name
   übernommen — in keinem Fall entsteht daraus ein Pfad oder ein
   Kommando.

6a. **Dieselben Namen, Exportseite.** Ein Server, dessen Name `\n`, `#`
   oder ein Anführungszeichen enthält, erzeugt beim Export **keine**
   zweite Zeile und keinen Kommentarabbruch; `ssh -F … -G` besteht
   (4.3). Getrennt von 6, weil für den Schreiber eigene Regeln gelten.
7. **Unsichtbare Randzeichen.** (a) Ein `HostName` mit BOM und
   Zero-Width-Space an den **Rändern** → die Zeichen sind entfernt, der
   Wert ist der erwartete (3.1.2, Absatz „Randzeichen"). (b) Dieselben
   Zeichen **innerhalb** des Werts → bleiben erhalten, der Wert weicht
   vom bereinigten ab. (c) Ein Wert, der **nur** aus solchen Zeichen
   besteht → gilt als leer, also als „nicht angegeben". Ohne (b) und (c)
   hätte der Test kein Soll und würde bei jeder Implementierung grün —
   ein Test ohne Soll.
8. **Der lokale Pseudo-Server als Jump-Host.** Eine Konfiguration, deren
   `ProxyJump` auf den Namen des lokalen Servers zeigt → abgelehnt mit
   dem Fehler „Jump-Host darf nicht der lokale Server sein" (5.7).
9. **Fremdinhalt erscheint nirgends, auch nicht in der Vorschau.** Vier
   Fälle, je mit `Include` auf eine Datei, die kein `ssh_config` ist:
   (a) ein privater Schlüssel im OpenSSH-Format; (b) eine Textdatei mit
   einer erkennbaren Zeichenfolge; (c) eine Binärdatei mit NUL-Bytes;
   (d) eine Datei mit ungültigem UTF-8. In **allen** Fällen gilt: Die
   erkennbare Zeichenfolge bzw. jeder Byte-Inhalt der Datei taucht
   **nirgends** auf — nicht in der Vorschau, nicht im Plan, nicht in
   einer Meldung, nicht im Log (5.4, 3.1.4a). Was auftaucht, ist genau
   zweierlei: der volle **Pfad** der Datei und der Satz
   „keine ssh_config, übersprungen".

9a. **Der Direktivenname verrät den Wert nicht.** Eine gültige
   `ssh_config` mit `Compression yes` und einer Zeile
   `UnknownDirective <geheimnisverdächtiger Wert>`. Die Liste der nicht
   übernommenen Direktiven enthält `Compression` und
   `UnknownDirective` mit Zeilennummern — und **keinen** der beiden
   Werte (3.1.5).

10. **`Match` als Blockgrenze und Klammern in Mustern.** `Host a` / `User x`, danach `Match host b` / `User y` ⇒ `a` hat Benutzer `x`. `Host web[1` ergibt einen wörtlichen Namen; `Host web[1-9]` trifft `web3` nicht. `Include` innerhalb von `Match` wird gemeldet, nicht gefolgt; Platzhalter in einer vorderen Pfadkomponente werden gemeldet, nicht aufgelöst; ein ungültiger `Port` fällt auf 22 zurück und erscheint unter „nicht übernommen".

