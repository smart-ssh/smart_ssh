# Spec 0071 — Schlüsselbund nicht verfügbar: klare Meldung statt englischem Backend-Text

Status: umgesetzt
Zweck: Ist der OS-Schlüsselbund beim Start nicht erreichbar (vor allem unter Linux ohne Secret Service), erfährt der Nutzer in seiner Sprache, was nicht geht, warum und welcher nächste Schritt hilft — statt eines englischen Bibliothekstextes.
Bezüge: Spec 0059 (Startfehler), Spec 0098 (Fehler zur Laufzeit), Spec 0101 (der Schlüsselbund hält nur noch den Wurzelschlüssel; Dialog D1), Spec 0094 (Logregeln), ADR 0093.
Review-Priorität: ERHÖHT (Credential-Handling)

## Entscheidungen

- Es geht um eine **Meldung**, nicht um einen Ersatzspeicher im Schlüsselbund-
  Modus. Ein Ersatz ohne Schlüsselbund ist das Master-Passwort aus Spec 0101.
- Der Startdialog wählt seine Sprache aus der Systemumgebung (A11a), und die
  Wahl gilt für alle Startdialog-Texte, auch die aus Spec 0059.
- Ein leerer API-Key schreibt kein Credential: nicht Teil dieser Spec.

## 1. Ausgangslage und Messungen

Die eingesetzte Schlüsselbund-Anbindung initialisiert ihren Speicher einmal
und faul. Schlägt das fehl, liefert jeder spätere Zugriff nur noch „No default
store has been set, so cannot search or create entries"; die eigentliche
Ursache ist dann nur noch über den Initialisierungsstatus des Speichers
abrufbar (billig, beliebig oft).

Gemessen auf Debian 13 (Container):

| Lage | Initialisierungsstatus |
|---|---|
| kein Session-Bus | Plattformfehler: Verbindung zum Bus-Socket nicht gefunden |
| Bus vorhanden, kein Anbieter | Plattformfehler: `org.freedesktop.secrets` wird von keinem Dienst angeboten |
| `gnome-keyring` entsperrt | Ok; Schreiben und Rücklesen gelingen |

Beide Fehlerlagen liefern denselben Fehlerzweig der Bibliothek; ein Text-
Vergleich könnte sie nicht trennen. Die Umgebung (A3) trennt sie korrekt.
Laufzeiten: Erstaufruf 4–40 ms, weitere unter 3 µs; es hängt nichts. Ein
gesperrter Anbieter ist nicht gemessen und gehört zu den manuellen Tests.

Paketnamen (Debian 13):

| Paket | Befund |
|---|---|
| `gnome-keyring` | richtig und ausreichend: einziges Paket im Archiv mit der D-Bus-Dienstdatei für `org.freedesktop.secrets`; startet per Aktivierung von selbst. `libpam-gnome-keyring` ist nur für das automatische Entsperren beim Login nötig. |
| `dbus-user-session` | richtig für den fehlenden Session-Bus. |
| `kwalletd6` | existiert nicht; der Daemon heißt `kwallet6` und meldet `org.kde.kwalletd5`/`…6`, nicht `org.freedesktop.secrets`. Installieren behebt die Lage nicht. |
| `keepassxc` | bringt keine D-Bus-Dienstdatei mit; der Name wird erst angemeldet, wenn die Anwendung läuft **und** die Secret-Service-Integration eingeschaltet ist (Vorgabe: aus). |

Offen bleibt, ob eine vollständige KDE-Plasma-Sitzung den Secret Service selbst
bereitstellt; bis dahin nennt der Text `gnome-keyring` als den Weg, der
nachweislich funktioniert (manuelle Tests M4).

## 2. Ziel und Nicht-Ziele

**Ziel:** Auf einem System ohne funktionierenden Schlüsselbund erfährt der
Nutzer in seiner Sprache (a) *was* nicht geht, (b) *warum*, (c) *welches Paket*
er installieren oder welchen Dienst er starten muss. Kein englischer
Bibliothekstext erreicht die Oberfläche.

Nicht-Ziele:

1. Kein Ersatz-Speicher im Schlüsselbund-Modus (Ersatz: Master-Passwort,
   Spec 0101).
2. Kein Starten oder Installieren von `gnome-keyring`, `kwalletd` oder
   `dbus-user-session` durch Smart SSH; die Anwendung gibt einen Befehl aus.
3. Kein geändertes Verhalten auf macOS und Windows — dort erscheinen keine
   Linux-Paketnamen.
4. Kein Wechsel der Schlüsselbund-Bibliothek oder des Backends.
5. Kein Freischalten des Fünf-Minuten-Pfads ohne Schlüsselbund.

## 3. Anforderungen

### Ursache klassifizieren

**A1** Aus dem Initialisierungsstatus wird ein Grund mit genau diesen Werten
bestimmt:

| Wert | Bedeutung |
|---|---|
| `NoSessionBus` | kein D-Bus-Session-Bus erreichbar (Linux) |
| `NoSecretServiceProvider` | Bus vorhanden, kein Anbieter unter `org.freedesktop.secrets` (Linux) |
| `Locked` | Anbieter vorhanden, aber gesperrt oder Abfrage abgewiesen |
| `Unknown` | alles andere, inklusive macOS und Windows |

**A2** Die Klassifizierung hängt nur von Parametern ab (Grund des Fehlers,
Betriebssystem, Session-Bus-Indiz), nicht vom Build-Ziel oder von der
Umgebung; sie ist ohne echten Schlüsselbund testbar.

**A3** Das Session-Bus-Indiz ermittelt der Aufrufer: `DBUS_SESSION_BUS_ADDRESS`
gesetzt und nicht leer, **oder** `$XDG_RUNTIME_DIR/bus` existiert. Das Indiz
steuert ausschließlich die **Wortwahl** eines Textes, nie eine
Sicherheitsentscheidung und nie, ob ein Secret geschrieben wird.

**A4** `Unknown` ist der Auffangfall und erzeugt einen vollständigen,
brauchbaren Text. Eine Fehlklassifikation führt nie dazu, dass gar keine
Meldung erscheint.

### Texte

**A5** Für jeden Wert aus A1 existiert ein Text mit (a) dem Zustand in einem
Satz, (b) dem, was dadurch nicht geht, (c) genau einem nächsten Schritt mit
konkretem Befehl. Für `NoSecretServiceProvider`: Installationsvorschlag ist
`sudo apt install gnome-keyring` und danach neu anmelden (das genügt unter
GNOME; für KDE ist es Inferenz, M4). In einem zweiten Satz stehen KWallet
(`kwallet6`) und KeePassXC als „falls ohnehin in Gebrauch" mit dem Hinweis,
dass ihre Secret-Service-Integration laufen bzw. eingeschaltet sein muss; für
sie gibt es keinen `apt install`-Befehl.

**A6** `NoSessionBus` nennt den Session-Bus als Ursache und `dbus-user-session`
als ersten Schritt — **nicht** die Schlüsselbund-Pakete, die dort nichts
helfen würden.

**A7** `Locked` fordert zum **Entsperren** auf und nennt **kein** zu
installierendes Paket und kein `apt`. Ein gesperrter Schlüsselbund wird nie als
fehlendes Paket dargestellt, damit niemand einen zweiten Anbieter neben einen
laufenden setzt.

**A8** Auf macOS und Windows enthält kein erzeugter Text einen Linux-
Paketnamen, einen `apt`-Befehl oder „Secret Service". Außerhalb von Linux wird
jeder Grund auf einen OS-neutralen Text abgebildet.

**A9** Der frühere Satz „Alle anderen Funktionen funktionieren normal" kommt
nicht vor, ebenso nicht „Smart SSH wird jetzt trotzdem gestartet": Seit Spec 0101
(E3) erscheint der Dialog, wo die Datenbank nicht geöffnet werden kann, und die
App startet daraus nicht. Kein Text behauptet, SSH-Verbindungen funktionierten
ohne erreichbaren Schlüsselbund weiter.

**A9a** Die Ursache nennt den Datenbankschlüssel, der im Schlüsselbund liegt,
nicht Passwörter, Passphrasen oder API-Keys: Seit Spec 0101 liegen die Secrets
verschlüsselt in der Datenbank, der Schlüsselbund hält nur deren Schlüssel. Die
Folgen nennen: Server, Passwörter, Passphrasen, Sudo-Passwörter, API-Keys,
Chat-Verlauf und Einstellungen sind nicht verfügbar. Der Dialog bietet
„Erneut versuchen" und „Beenden", ggf. das Einrichten eines Master-Passworts
(Spec 0101, E3/E8).

**A9b** Der Zustand wird einmal beim Start ermittelt und danach nie verändert;
es gibt keine nachträgliche Eskalation von „verfügbar" auf „nicht verfügbar"
und keine zusätzliche Startwarnung, wenn ein späterer Zugriff scheitert. Das
Ergebnis erscheint in den Einstellungen als beim Start ermittelter Zustand.
Scheitert ein Zugriff, obwohl der Schlüsselbund beim Start da war, gilt Spec
0098 (`KEYCHAIN_ACCESS_FAILED`).

**A10** Kein Text enthält einen rohen Bibliotheksfehler, eine D-Bus-Adresse,
einen Benutzernamen oder einen Pfad, der nicht vorher durch eine
Steuerzeichen-Bereinigung gelaufen ist.

**A11** Alle Texte liegen in DE und EN vor; neue Schlüssel stehen in beiden
Sprachdateien der Oberfläche.

**A11a** Der Startdialog läuft vor der Oberflächen-Sprachwahl und wählt seine
Sprache aus der Systemumgebung: erste gesetzte, nicht leere Variable aus
`LC_ALL`, `LC_MESSAGES`, `LANG`; ausgewertet wird nur das Präfix vor `_`, `.`
oder `@` (`de_DE.UTF-8` → `de`). Beginnt es mit `de` → Deutsch, sonst Englisch.
Ist keine Variable gesetzt oder der Wert unbrauchbar (`C`, `POSIX`, leer), gilt
Deutsch.

**A11b** Die Sprachwahl gilt für **alle** Startdialog-Texte, auch die aus
Spec 0059 (Datenbankfehler, Host-Key-Speicher, Schlüsselbund). Die EN-Fassungen
sind Übersetzungen; insbesondere bleibt die Warnung beim Host-Key-Speicher, dass
eine erneute Erstbestätigung **keinen** Schutz vor einem untergeschobenen Server
bietet, in beiden Sprachen gleich deutlich.

**A11c** Die Sprachwahl ist eine reine Funktion über den Umgebungswert.

### Verdrahtung

**A12** Der Startdialog verwendet die Textwahl mit dem klassifizierten Grund;
bei einem nicht erreichbaren Schlüsselbund ist das die Ursache-Zeile des
Dialogs D1 (Spec 0101).

**A13** Ein Backend-Fehler aus einem nicht verfügbaren Schlüsselbund erreicht
die Oberfläche mit dem stabilen Code `KEYCHAIN_UNAVAILABLE`; die Oberfläche
zeigt den übersetzten Text, nicht die Nutzlast. Der Code ersetzt die Meldung,
er ergänzt sie nicht. (Zur Laufzeit gilt Spec 0098.)

**A14** Ob ein Sudo-Passwort hinterlegt ist, unterscheidet „nicht vorhanden"
von „Speicher nicht lesbar". Im zweiten Fall behauptet die Oberfläche nicht
„kein Sudo-Passwort", sondern zeigt einen neutralen Zustand.

**A15** Die Diagnose-Ansicht zeigt eine Zeile „Systemschlüsselbund:
verfügbar / nicht verfügbar (Grund)", damit der Zustand auch nachschlagbar ist,
wenn der Startdialog weggeklickt wurde.

**A16** Der Zustand wird **einmal pro Programmlauf** ermittelt und gehalten.
Kein Kommando probiert den Schlüsselbund zusätzlich ab, um den Zustand zu
erfahren — insbesondere nicht beim Laden der Serverliste.

**A17** Ein vom Nutzer **ausgelöstes** Entfernen eines Secrets meldet nie
Erfolg, wenn es nicht entfernt werden konnte:

- „Hinterlegtes Sudo-Passwort entfernen" schlägt sichtbar fehl. (Zur Laufzeit
  meldet der Weg `KEYCHAIN_ACCESS_FAILED`, wenn der Schlüsselbund beim Start
  da war, siehe Spec 0098 A1; dass er **sichtbar** fehlschlägt, bleibt.)
- „Server löschen" läuft **durch**: Das Profil wird gelöscht, auch wenn das
  Secret bleibt. Das Ergebnis sagt ausdrücklich, dass das Secret nicht
  entfernt werden konnte, mit dem Hinweis, dass der Eintrag verwaist ist und
  von Hand gelöscht werden kann.
- In beiden Fällen bleibt die Warnung im Log.

## 4. Verhalten bei Fehlern

| Wo es scheitert | Was der Nutzer sieht |
|---|---|
| Start, Wurzelschlüssel nicht erreichbar | Dialog D1 (Spec 0101) mit Text nach A5–A8 |
| Provider anlegen, ändern, löschen | Fehler im Provider-Dialog, übersetzter Code |
| Server-Passwort oder Passphrase speichern | Fehler im Server-Formular, gleicher Text |
| Sudo-Passwort setzen | Fehler im Sudo-Dialog, gleicher Text |
| Zustand nachschlagen | Diagnose-Ansicht (A15) |

Die Degradierung bei Fehlern des Wurzelschlüssels (Spec 0040 Abschnitt 7) und
die Nicht-Fatalität aus Spec 0059 bleiben: Diese Spec ändert, *was* gemeldet
wird, nicht *ob* gestartet wird.

## 5. Sicherheitszusagen

- **I1 Kein Secret in einer Meldung.** Die Textwahl nimmt nur den Grund, das
  Betriebssystem und die Sprache entgegen, nie ein Secret, einen Verweis oder
  einen rohen Backend-Fehler.
- **I2 Kein stiller Rückfall.** Ist der Schlüsselbund nicht verfügbar, scheitert
  jeder Schreibzugriff sichtbar; es entsteht kein zweiter Speicherort und kein
  „merke es dir für diese Sitzung".
- **I3 Kein falsches Erfolgssignal.** Ein fehlgeschlagener Schreibzugriff führt
  nie zu einem angelegten Profil ohne Secret.
- **I4 Unbekannt ist nicht „nein"** (A14).
- **I5 Keine Empfehlung, die Sicherheit senkt.** Kein Text rät, Secrets in einer
  Datei, einer Umgebungsvariable oder einem passwortlosen Schlüsselbund
  abzulegen; ein leeres Passwort wird nur als ausdrückliche Warnung erwähnt.
- Nicht berührt: Filter, Risiko, Redaction, Ausführungspfad.

## 6. Testfälle

### 6.1 Klassifizierung

- T1 Grund „nicht erreichbar" ohne Session-Bus-Indiz → `NoSessionBus`.
- T2 Derselbe Grund mit Indiz → `NoSecretServiceProvider`.
- T3 Grund „Zugriff verweigert/gesperrt" → `Locked`, unabhängig vom Indiz.
- T4 Erfolgreicher Status → keine Meldung.
- T5 Ziel-OS macOS/Windows → `Unknown`, unabhängig vom Indiz.

### 6.2 Texte

- T6 `NoSecretServiceProvider` (Linux) nennt `gnome-keyring` und einen
  Installationsbefehl.
- T7 `NoSessionBus` nennt den Session-Bus und **nicht** `gnome-keyring` als
  ersten Schritt.
- T8 `Locked` enthält kein `apt` und kein „install".
- T9 macOS und Windows enthalten weder `gnome-keyring` noch `apt` noch
  „Secret Service".
- T10 Jeder Text beschreibt, was nicht geht, und enthält nicht mehr „funktionieren
  normal".
- T11 Kein Text enthält „trotzdem gestartet" und behauptet nicht, die App starte
  weiter oder SSH-Verbindungen liefen weiter (A9, Spec 0101 E3); die Ursache
  nennt den Datenbankschlüssel im Schlüsselbund (A9a).
- T11a Sprachwahl: `de_DE.UTF-8`, `de`, `de_AT@euro` → Deutsch; `en_US.UTF-8`,
  `fr_FR`, `ja_JP.UTF-8` → Englisch; `C`, `POSIX`, `""`, nicht gesetzt →
  Deutsch.
- T11b Für **jeden** Startdialog-Text existieren DE und EN, nicht identisch.
- T11c Die EN-Fassung des Host-Key-Hinweises enthält die „kein Schutz"-Warnung
  ebenso ausdrücklich wie die deutsche.
- T12 Kein erzeugter Text enthält einen Platzhalter-Secret-Wert.

### 6.3 Adversarial

- **X1** Ein Steuerzeichen in der Umgebung (`\n`/`\r` in der Bus-Adresse) setzt
  den Dialogtext nicht optisch fort; bevorzugt wird der Wert gar nicht
  ausgegeben. Die Bereinigung ersetzt außer Steuerzeichen auch Zeilen- und
  Absatztrenner (U+2028, U+2029) und Textrichtungs-Steuerzeichen (U+200E,
  U+200F, U+202A–U+202E, U+2066–U+2069) durch `?`; gewöhnliche
  Nicht-ASCII-Zeichen bleiben unverändert. Dasselbe gilt für Dateinamen in den
  Wahldialogen und für Nutzereingaben, die ein Serverformular als ungültig
  zitiert (Pfad des SFTP-Servers, Startverzeichnis).
- **X2** Ein Backend-Fehler, dessen Text einen Platzhalter wie `sk-live-hunter2`
  enthält, taucht in keinem erzeugten Text und in keiner Fehlermeldung mit Code
  `KEYCHAIN_UNAVAILABLE` auf.
- **X3** Ein gesperrter, vorhandener Schlüsselbund schickt den Nutzer nie zu
  `apt install`.
- **X4** Ein Server mit hinterlegtem Sudo-Passwort und einem Speicher, der einen
  Backend-Fehler liefert, meldet nicht `has_sudo_password: false` (A14).
- **X5** Kein Text rät zu einem Klartext-Ablageort oder einem passwortlosen
  Schlüsselbund; die Begriffe „Klartext", „plain text", „ohne Passwort",
  „empty password" kommen nur mit begleitendem Warnwort vor.
- **X6** Kein Codepfad gibt bei nicht verfügbarem Speicher Erfolg zurück: alle
  Schreibzugriffe reichen ihren Fehler weiter. Ausgenommen sind echte
  Aufräumpfade, die einen bereits fehlgeschlagenen Vorgang zurückbauen (dort ist
  der ursprüngliche Fehler die relevante Information), nicht aber vom Nutzer
  ausgelöstes Entfernen (A17).

### 6.4 Manuelle Tests

- M1 Debian-Minimal ohne Keyring und Session-Bus: Dialogtext nach A6, App
  läuft, Log enthält den vollen Grund.
- M2 Mit `dbus-user-session`, ohne Anbieter: Text nach A5; Provider anlegen →
  übersetzter Fehler, kein englischer Bibliothekstext.
- M3 `gnome-keyring` installieren, neu anmelden: Provider anlegen gelingt, kein
  Dialog, Diagnose-Zeile „verfügbar".
- M4 KDE-Sitzung mit gesperrtem KWallet: Text nach A7, kein `apt`-Befehl;
  prüfen, ob `gnome-keyring` unter KDE den Secret Service tatsächlich behebt.
- M5 macOS mit abgebrochenem Keychain-Dialog: keine Linux-Paketnamen.
- M6 `LANG=en_US.UTF-8` und `LANG=de_DE.UTF-8`: derselbe Dialog in der
  jeweiligen Sprache; ein herbeigeführter Datenbankfehler unter `en_US.UTF-8`
  ebenfalls englisch.

## 7. Grenzen

- Das Session-Bus-Indiz ist eine Heuristik; liegt es falsch, nennt der Text das
  falsche der beiden Pakete, was für die Sicherheit folgenlos ist.
- Eine vollständige Plasma-Sitzung ist nicht gemessen (M4).
