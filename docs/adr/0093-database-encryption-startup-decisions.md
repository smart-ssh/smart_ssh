# ADR 0093 — Entscheidungen beim Verschlüsseln der Datenbank und beim Start

Status: angenommen
Bezug: Spec 0101 (Etappe 1, Commits 3–5), Spec 0036, Spec 0040, Spec 0059,
Spec 0071, Spec 0098, ADR 0092

Spec 0101 lässt für Etappe 1 mehrere Punkte offen oder wird vom Code
bewusst anders als wörtlich gelesen. Das hier ist das Protokoll dazu.

## 1. `resolve_or_generate_key` wurde entfernt, nicht nur nicht mehr gerufen

A3 verlangt „nie still ein neuer K". Die vorhandene Funktion erzeugte bei
`NotFound` selbst einen Schlüssel und speicherte ihn. Sie durch
`read_root_key` (liest nur) und `generate_and_store_root_key` (erzeugt
ausdrücklich) zu **ersetzen** statt stehen zu lassen, war eine Entscheidung
mit Preis: Spec 0036 nennt die alte Funktion namentlich, und ihre Tests
mussten umgeschrieben werden.

Begründung: Eine Funktion, die von sich aus einen Schlüssel erzeugt, ist
die Angriffsrichtung „ein Weg, der doch einen neuen K erzeugt" in Code
gegossen. Solange sie existiert, hängt die Invariante daran, dass niemand
sie aufruft — und das ist keine Zusicherung, sondern eine Hoffnung.

## 2. Spec 0059 Fall 3 entfällt ersatzlos

Fall 3 war: Schlüsselbund gesperrt → sichtbare Warnung, App startet
**degradiert** weiter (ohne Chatverlauf, ohne Historie, ohne Ledger).
Diesen Zustand gibt es nicht mehr. Ohne K lässt sich die verschlüsselte
Datenbank überhaupt nicht öffnen; der Start endet in Dialog D1 („Erneut
versuchen" / „Beenden").

Das ist strenger, nicht nachlässiger: Es gibt keinen halb benutzbaren
Zustand mehr, in dem unklar bleibt, was gerade geschrieben wird und was
nicht. `app_shell::startup_dialog::show_warning` bleibt mit
`#[allow(dead_code)]` stehen, weil Etappe 3 (A11.1, A17) wieder
nicht-fatale Starthinweise braucht.

Folge, die mit aufgeräumt werden muss: `credentials_keyring::
escalate_to_unavailable` und `app_logic::startup_error_messages::
should_warn_about_keychain` haben keinen Produktivaufrufer mehr. Spec 0071
A13/A15 beschreibt damit eine Eskalation, die so nicht mehr stattfindet.
**Nicht in diesem Lauf angefasst** — das ist eine Änderung an Spec 0071,
nicht an 0101.

## 3. Der Symlink-Abbruch gilt für *fehlt* und *Klartext*, nicht für *sonst*

A6 verbietet wörtlich nur die **Umwandlung** eines Symlinks.
`detect_database_file_state` folgt dem Link aber, und eine Verknüpfung auf
ein nicht vorhandenes Ziel las sich als *fehlt* — `create_if_missing` legte
die neue, verschlüsselte Datenbank dann am Ziel der Verknüpfung an,
außerhalb des 0700-Datenverzeichnisses. Die Regel aus A6 gilt deshalb für
jeden Weg, auf dem eine Datei **angelegt oder umbenannt** wird.

Für *sonst* (eine bewusst gesetzte Verknüpfung auf eine bereits
verschlüsselte Datenbank) gilt sie **nicht**: Dort wird nur geöffnet, es
entsteht keine Datei, und der Dialogtext („wird nicht automatisch
verschlüsselt, weil das die Verknüpfung ersetzen würde") wäre für diesen
Fall die falsche Begründung. Eine erste Fassung lehnte jeden Symlink ab;
das war eine Verschärfung ohne Anlass und ist zurückgenommen.

## 4. Der Verlusthinweis in D1 hängt am Einrichten-Knopf

A3/D1 sagt: „Bei *Klartext* nennt der Text den Verlust des bisherigen
Verlaufs wie D4." Der Code zeigt diesen Satz nur, wenn D1 zugleich
„Master-Passwort einrichten" anbietet.

Das ist eine **Abweichung vom Wortlaut von A3**, bewusst und mit dieser
Lesart: Der Satz warnt vor dem Verlust, den das Einrichten verursacht
(dabei entsteht ein neuer K). Ohne diesen Knopf passiert in D1 überhaupt
nichts — die Wahl ist „erneut versuchen" oder „beenden" —, und ein Hinweis
auf einen Verlust neben dem Satz „An deinen Daten wurde nichts verändert"
wäre für den Nutzer widersprüchlich. In Etappe 1/2 gibt es den Knopf noch
nicht, der Satz erscheint also derzeit nie.

Der Rechenweg steht trotzdem schon in `decide_startup`
(`offers_password_setup`) und ist für alle zwölf Felder der Tabelle
getestet, damit Etappe 3 nur noch den Knopf zeichnen muss.

## 5. Der Datenbankschlüssel wurde über `sqlx`s Statement-Log geschrieben

Das war der blockierende Fund des Reviews, und er ist hier festgehalten,
weil die Ursache nicht offensichtlich ist: `sqlx` fasst alle Pragmas einer
Verbindung zu **einer** Anweisung zusammen und führt sie durch seinen
normalen `QueryLogger`. Dessen Vorgaben sind `DEBUG` für jede Anweisung und
`WARN` für eine, die länger als eine Sekunde braucht. Der vollständige
Schlüssel stand damit mit `RUST_LOG=debug` bei jedem Verbindungsaufbau und
beim Standard-Loglevel `info` auf dem Slow-Statement-Pfad in der Logdatei.

Zwei Dinge daran sind für die Zukunft wichtig:

- Die Redaktion auf dem **Fehlerweg** (`redact_if_key_bearing`) greift dort
  nicht. Eine Erfolgsmeldung ist kein Fehler. Wer Schlüsselmaterial an eine
  Bibliothek übergibt, muss deren Log-Weg getrennt schließen.
- Auch der Pool darf die Verbindung nicht zur Laufzeit ersetzen
  (`idle_timeout`, `max_lifetime`, `test_before_acquire`): Jeder Neuaufbau
  führt den Schlüssel-Pragma-Block erneut aus, dann aber außerhalb von
  `connect_encrypted` und damit außerhalb jeder Redaktion.

Erkannt wird Schlüsselmaterial in einem Fehlertext an jedem Fenster von
8 Hex-Zeichen. Ein kürzeres Zitat (bis 7 Zeichen, 28 Bit) geht bewusst
durch — darunter wäre die Falschmeldungsrate nicht mehr vernachlässigbar,
und ein Fehler, der seine diagnostische Variante verliert, kostet Spec 0059
mehr, als 28 Bit Schlüsselpräfix einbringen.

## 6. `connect_plaintext` liegt hinter `test-support`

Der Weg, der die Datenbank **unverschlüsselt** öffnet, ist nur unter dem
Feature `test-support` sichtbar. `app-logic` und `app-shell` aktivieren es
ausschließlich unter `[dev-dependencies]`; durch `resolver = "2"` werden
diese Features bei `cargo build --workspace` nicht unifiziert, ein
Produktivaufruf scheitert dort also an der Kompilierung.

Die Einschränkung gehört dazu: `cargo test` und
`cargo clippy --all-targets` haben das Feature **an**. Der Schutz hängt am
vollständigen Gate, namentlich an seinem letzten Schritt.

## 7. Bewusst nicht behobene Review-Funde

- **Kein Single-Instance-Schutz.** Zwei gleichzeitig gestartete Instanzen
  können beide eine Umwandlung derselben Klartext-Datei beginnen. Dass
  dabei nichts Halbes übernommen wird, hängt allein an der Prüfung in
  Schritt 3 (`integrity_check`, Zeilenzahlen je Tabelle,
  `_sqlx_migrations`, `user_version`, `journal_mode`). Spec 0101 §1 nimmt
  „Kein anderer Prozess öffnet die Datei" als gegeben an; ein Lockfile ist
  eine repo-weite Änderung und gehört nicht in diese Spec.
- **A19 (Schlüsselmaterial in überschreibenden Typen) ist offen.**
  `OpenedDatabase.root_key` und der Pragma-Wert in den
  `SqliteConnectOptions` sind nicht überschreibende `String`/`[u8; 32]`.
  A19 ist SOLL und laut Umsetzungsreihenfolge Teil von Commit 9.
- **Das Feld *Klartext* × *NotFound* erzeugt ohne Meldung einen neuen K**
  und macht damit einen eventuell vorhandenen, feldweise verschlüsselten
  Verlauf unlesbar. Spec-gedeckt („K erzeugen, umwandeln"), aber der
  einzige Fall, in dem genau derselbe Verlust ohne den D4-Hinweis passiert.
  Empfehlung für ein eigenes Item.
- **Eine Namenskollision beim „Neu anfangen"** endet sichtbar, aber mit dem
  `Other`-Text, der zu einer Backup-Wiederherstellung rät. Ein eigener Text
  dafür ist Nacharbeit, kein Mangel an der Invariante („löscht nichts"
  gilt).
- **`app-logic`s eigenes `test-support`-Feature leitet
  `persistence-sqlite/test-support` nicht weiter.** Fällt erst auf, sobald
  ein Helfer außerhalb von `#[cfg(test)]` `connect_plaintext` braucht.

## 8. Etappe 2 (Commits 6–8) ist in diesem Lauf nicht umgesetzt

Der Auftrag dieses Laufs umfasste die Commits 3–8. Umgesetzt sind 3–5
(Etappe 1) samt zwei Review-Runden und deren Nacharbeit. Die Commits 6–8
(Secrets-Store in der Datenbank, Umzug aus dem Schlüsselbund, MCP-Token)
sind **nicht** begonnen.

Das ist eine Scope-Reduktion und hier ausdrücklich vermerkt. Der Grund ist
nicht fachlich: Das Budget des Laufs reichte nach zwei Review-Runden mit
einem blockierenden Sicherheitsfund nicht mehr für drei weitere Commits an
sicherheitskritischem Credential-Code **plus** die dafür nötige Prüfung.

Die Teilung ist an einer unschädlichen Stelle gemacht: Commit 6 allein
würde `AppState.credential_store` auf die Datenbank umstellen, **bevor**
Commit 7 die vorhandenen Secrets dorthin umzieht — jede bestehende
Installation verlöre damit den Zugriff auf ihre gespeicherten Passwörter.
Die Commits 6 und 7 gehören deshalb in denselben Lauf. Nach Commit 5
steht der Code dagegen auf einem in sich geschlossenen Stand: Die
Datenbank ist verschlüsselt, Secrets liegen weiter im Schlüsselbund, und
nichts daran ist halb fertig.

**Offen bleibt damit auch Teil 0 Frage 2** (synchroner `CredentialStore`
auf asynchronem Pool, laut Spec vor Commit 6 zu klären).
