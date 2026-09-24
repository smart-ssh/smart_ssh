# ADR 0065: Entscheidungen bei der Umsetzung von Spec 0076 (Anmeldung mit einer Schlüsseldatei)

Status: Angenommen
Bezug: docs/specs/0076-identity-file-auth.md, Commits `8062078` (Variante,
Schnittstelle, Kette, Leseumsetzung), `f3d13c3` (DTO-Pfad, Hop-Angabe),
`489718c` (Überführung), `d0cc3a4` (§9 K-3, `libc::O_NONBLOCK`), `28b14fd`
(Schritt 5, Frontend)

Spec 0076 §8 sagt „Offene Punkte: Keine". Beim Umsetzen sind trotzdem
Festlegungen aufgelaufen, die die Spec nicht trifft — und zwei davon
weichen von ihr ab. Sie stehen hier, damit sie sichtbar bleiben statt in
einer Commit-Message zu versanden.

Die Schritte 1 bis 4 aus §7 sind unter §1–§14 protokolliert. §15 gehört zu
Schritt 5 (Frontend); Schritt 6 (`CHANGELOG.md`-Fragment) brauchte keine
eigene Entscheidung.

## 1. Schritt 1 und Schritt 2 sind **ein** Commit geworden (Abweichung von §7)

**Frage:** §7 schneidet Schritt 1 („Variante, Schnittstelle, Kette") und
Schritt 2 („Umsetzung des Lesens") als zwei Schritte, und §7 verlangt
zugleich, dass jeder Schritt für sich committbar ist und das Gate grün
lässt.

**Befund:** Beides zusammen geht nicht. Schritt 1 verlangt ausdrücklich
„das neue `AppState`-Feld" (§7.1, vierter Punkt). Ein `AppState`-Feld vom
Typ `Arc<dyn KeyFileReader + Send + Sync>` braucht eine **konkrete**
Umsetzung, um gebaut werden zu können — und die ist Schritt 2. Ein Schritt 1
ohne Schritt 2 übersetzt also nicht, es sei denn, man schöbe eine Attrappe
in den Produktionspfad, die jeden Zugriff mit einer unwahren Meldung
ablehnt. Genau das wäre der stille Zwischenstand, den §7 vermeiden will.

**Entscheidung:** Beide Schritte liegen in Commit `8062078`, zusammen mit
ihren Tests (§6.1, §6.2, §6.4.2/3/6/7, §6.4.8). Die Reihenfolge innerhalb
des Commits folgt der Spec; nur die Commit-Grenze fällt weg.

**Folge für den Review:** Der Commit ist groß (rund 2 200 Zeilen). Das ist
der Preis dafür, dass es keinen Zwischenstand gibt, der nicht übersetzt.

## 2. `classify_openssh_key` liegt in `ssh-transport`, nicht in `app-shell`

**Frage:** §4.2 sagt „Die konkrete Umsetzung liegt in `app-shell`". Die
Umsetzung muss den gelesenen Schlüssel auf **Gültigkeit** prüfen und
melden, ob er **verschlüsselt** ist (§4.2, letzter Absatz). `app-shell` hat
keine Schlüssel-Bibliothek.

**Entscheidung:** `OsKeyFileReader` liegt wie verlangt in `app-shell`
(`crates/app-shell/src/key_files.rs`). Nur die Gültigkeitsprüfung selbst
liegt als `ssh_transport::classify_openssh_key` dort, wo `russh` schon
wohnt — unmittelbar neben `load_private_key`, das dieselbe
`PrivateKey::from_openssh`-Zeile benutzt.

**Warum nicht anders:** `app-shell` eine Schlüssel-Bibliothek zu geben wäre
eine neue Abhängigkeit für eine Zeile, die es an anderer Stelle schon gibt
— und neue Abhängigkeiten sind ohne Freigabe ausgeschlossen. `core`
scheidet ohnehin aus (§1.3). Nebeneffekt, der für diese Aufteilung spricht:
Schlüssel werden weiterhin an genau einer Stelle geparst.

**Was dabei nicht verwässert wurde:** `classify_openssh_key` gibt den
Fehlertext der Bibliothek **nicht** heraus (§5.2) — er wird verworfen, und
der Aufrufer formuliert seine eigene, pfadtragende Meldung.

## 3. `O_NONBLOCK` wird selbst gehalten statt über `libc` bezogen

**Frage:** A-3 verlangt `O_NONBLOCK` beim Öffnen unter Unix
(„nicht verhandelbar"). Rusts Standardbibliothek gibt die Konstante nicht
heraus; `libc` ist im Arbeitsbereich nirgends direkte Abhängigkeit.

**Entscheidung:** Die Konstante steht per `#[cfg(target_os = …)]` in
`crates/app-shell/src/key_files.rs`. Linux/Android `0o4000`, die
BSD-Familie inklusive macOS `0x0004`. **Keine neue Abhängigkeit.**

**Warum vertretbar:** Der Wert wird nicht behauptet, sondern **gemessen**.
`test_named_pipe_is_rejected_without_blocking` öffnet ein benanntes Rohr
ohne Schreiber unter einem 10-Sekunden-Timeout. Stimmte die Konstante
nicht, bliebe das Öffnen hängen und der Test würde rot — nachgewiesen durch
einen Gegenbeweis (Flag entfernt → Test scheitert mit „Timeout").

**Was daran unschön bleibt, und wie es scheitert:** Auf einem Unix-Target,
das von keinem der `#[cfg]`-Zweige erfasst ist, **übersetzt die Crate
nicht** (die Konstante fehlt). Das ist Absicht: Ein lautes
Compile-Scheitern ist bei einem sicherheitsrelevanten Flag die richtige
Fehlerart, eine stille 0 wäre die falsche. Die unterstützten Zielsysteme
(macOS, Linux, Windows) sind abgedeckt.

**Nachtrag nach dem Review:** Die erste Fassung staffelte allein nach
`target_os`. Der Wert ist aber auf Linux **architekturabhängig** — `mips`
benutzt `0o200`, `sparc64` `0x4000`. Dort hätte der Code übersetzt und ein
**falsches** Flag gesetzt; das Fehlerbild wäre genau der von A-3 verbotene
Hänger gewesen, nicht der oben versprochene Übersetzungsfehler. Der
Linux-Zweig zählt deshalb jetzt die Architekturen auf, für die `0o4000`
gilt. Was nicht aufgezählt ist, übersetzt nicht — damit stimmt die Zusage
oben wieder.

**Vorgelegt, nicht entschieden:** Wer `libc` lieber als direkte
Abhängigkeit hätte, ändert drei Zeilen. Diese Umsetzung nimmt die
Abhängigkeit nur deshalb nicht auf, weil die Regel „keine neuen
Abhängigkeiten ohne Freigabe" gilt — nicht, weil `libc` hier sachlich
falsch wäre.

**Nachtrag (Spec 0076 §9 K-3, 2026-09-24):** Stefan hat die Aufnahme
freigegeben. `libc = "0.2.189"` ist jetzt direkte Abhängigkeit von
`app-shell` (`crates/app-shell/Cargo.toml`); dieselbe Version, die vorher
schon transitiv im `Cargo.lock` stand — kein neues Paket in der
Lieferkette. `open_readable` benutzt `libc::O_NONBLOCK`, die beiden
`#[cfg(target_os = …)]`/`#[cfg(target_arch = …)]`-Konstanten oben sind
entfallen. `test_named_pipe_is_rejected_without_blocking` (der Nachweis für
den Wert der Konstante) und `test_an_endless_source_is_never_read_beyond_the_limit`/
`test_the_read_limit_holds_even_when_fstat_understates_the_size` (die
Lesegrenzen-Tests) blieben dabei unverändert grün — erneut per Gegenbeweis
geprüft: `.custom_flags(libc::O_NONBLOCK)` entfernt, FIFO-Test scheitert
nach 10 s mit „Timeout" statt zu hängen, Fix wiederhergestellt, Test wieder
grün.

Die „Was daran unschön bleibt"-Warnung oben (stilles Übersetzungsscheitern
auf einer nicht aufgezählten Architektur) gilt seitdem nicht mehr: `libc`
deckt die Zielarchitekturen der Kiste selbst ab, es gibt keine von Hand
gepflegte Aufzählung mehr, die veralten könnte.

## 4. Die Rechteprüfung folgt OpenSSH wörtlich (`st_mode & 077`), nicht der Formulierung von A-4

**Frage:** A-4 sagt „für Gruppe oder Welt **lesbar**". E-1 sagt „genauso
ablehnen wie `ssh`". OpenSSHs `sshkey_perm_ok` prüft `(st.st_mode & 077)
!= 0`, also **jedes** Recht für Gruppe oder Welt — auch ein reines
Schreibrecht ohne Leserecht.

**Entscheidung:** `(mode & 0o077) != 0`, also die OpenSSH-Regel. Sie ist
strenger als die Formulierung in A-4 und in keinem Fall laxer. E-1 hat
Vorrang: „Wir sind laxer als `ssh`" ist bei diesem Produkt die Fassung, die
nicht aufschreibbar wäre.

Nachgewiesen durch `test_group_write_only_is_also_rejected_like_openssh_does`
— eine Datei mit `0620` wird abgelehnt. Der Test scheitert, sobald jemand
die Prüfung auf `0o044` verengt.

## 5. Ein leerer Pfad wird abgewiesen, ein sonstiger Pfad nicht angefasst

**Frage:** A-1 verlangt, den Pfad zu speichern, „wie der Nutzer ihn
angegeben hat" — kein `realpath`, keine Normalisierung. Formularfelder
liefern aber auch Leerstrings und Copy-Paste-Reste.

**Entscheidung:** Der Pfad wird **nicht** getrimmt. Einzige Ausnahme ist
die Leer-Prüfung: Ein Pfad, der nach `trim()` leer ist, ergibt
`SERVER_IDENTITY_FILE_REQUIRED`. Gespeichert wird in jedem anderen Fall der
unveränderte Eingabewert.

**Warum:** Trimmen wäre eine stille Normalisierung, die A-1 ausschließt —
und ein Pfad, der auf ein Leerzeichen endet, ist unter Unix ein gültiger
Pfad. Ein Nutzer, der aus Versehen ein `\n` mitkopiert, bekommt dafür eine
Meldung, die den Pfad nennt (A-6), statt einer stillen Korrektur.

**Abweichung von einer Nachbarkonvention:** Passwort, Passphrase und
Sudo-Passwort werden rand-getrimmt (Spec 0049 Fund 1, Spec 0073 A3). Der
Pfad ist bewusst anders behandelt, weil er kein Secret ist, sondern eine
Adresse.

## 6. Die Hop-Angabe (A-8) hängt nur an `CredentialResolutionFailed`

**Frage:** A-8 verlangt, dass die Fehlermeldung sagt, welcher Hop
gescheitert ist. Welche Fehler das betrifft, sagt die Spec nicht.

**Entscheidung:** `ssh_transport::auth::name_hop` reichert **nur**
`SshError::CredentialResolutionFailed` an — mit `benutzer@host:port` vor
der bisherigen Meldung. Jede andere Variante bleibt unberührt.

**Warum:** Nur diese Variante trägt einen Text, in den etwas hineinpasst,
und nur sie entsteht je Hop auf eine Weise, die sonst mehrdeutig bliebe.
`AuthenticationFailed` hat keinen Payload; Verbindungs- und
Host-Key-Fehler tragen Host und Port ohnehin schon.

**Was dabei stabil bleibt:** `SshError::code()` ändert sich nicht. Das
Frontend-Mapping (Spec 0024, Abschnitt 5) hängt am Code, nicht am Text —
das prüft `test_t6_3_6_a_failing_identity_file_names_the_hop` ausdrücklich
mit.

## 7. §6.3.7/§6.3.8 laufen gegen eine einfache Lese-Attrappe, nicht gegen `OsKeyFileReader`

**Frage:** §6.3.7 („falsche Passphrase") und §6.3.8 („verschlüsselter
Schlüssel mit richtiger Passphrase verbindet") brauchen einen echten
SSH-Server. Der steht als Fixture in `crates/ssh-transport/tests/`.
`OsKeyFileReader` liegt in `app-shell`, und `app-shell` hängt von
`ssh-transport` ab — andersherum geht es nicht.

**Entscheidung:** Die beiden Integrationstests benutzen `PlainFileKeyReader`,
eine bewusst ungehärtete Attrappe (`std::fs::read` +
`classify_openssh_key`). Geprüft wird dort die **Anmeldehälfte**: löst
`resolve_auth` die Passphrase richtig auf, und was sagt die Meldung, wenn
sie falsch ist.

**Was damit nicht geprüft ist, und wo es geprüft wird:** Die Härtungen aus
A-3/A-4 (`O_NONBLOCK`, `fstat` auf dem Handle, Pfadform, Rechte, Größe)
laufen **nicht** durch diese beiden Tests. Sie sind durch §6.2 und
§6.4.2/3/6/7 in `crates/app-shell/src/key_files.rs` abgedeckt — gegen echte
Dateien, mit Gegenbeweisen. §6.4.4 benutzt zusätzlich den echten
`OsKeyFileReader` gegen eine echte Datei.

**Was das offenlässt:** Es gibt keinen Test, der den gehärteten Leser und
einen echten SSH-Server in einem Lauf verbindet. Wer das will, müsste
`OsKeyFileReader` nach `ssh-transport` verschieben — das widerspräche §4.2
und ist deshalb nicht selbst entschieden worden.

## 8. Der Rollback der Überführung stellt wieder her, statt zu löschen

**Frage:** C-6 verlangt, dass ein fehlgeschlagenes Speichern des Servers
den eben geschriebenen Schlüsselbund-Eintrag zurücknimmt. Was, wenn unter
dem Slot schon etwas stand?

**Entscheidung:** `identity_file::roll_back_key_slot` merkt sich den
vorherigen Wert und stellt ihn wieder her; nur wenn es keinen gab, wird der
Slot geleert.

**Warum über die Spec hinaus:** Ein `IdentityFile`-Server zeigt nie auf
`private_key`, dort sollte also nichts liegen. „Sollte" ist aber keine
Zusicherung, und ein Rollback, der fremde Daten entfernt, ist kein
Rollback. Der Fall kostet drei Zeilen und einen Test
(`test_a_rollback_restores_a_pre_existing_slot_value_instead_of_deleting_it`).

## 9. Testschlüssel werden erzeugt, nicht eingebettet

**Frage:** §6.2 und §6.4.2 brauchen echte OpenSSH-Schlüssel — gültige,
verschlüsselte, öffentliche und abgeschnittene.

**Entscheidung:** `ssh_transport::test_keys` erzeugt sie zur Testlaufzeit,
hinter dem neuen Feature `test-support` (aktiviert nur in
`app-shell`s `[dev-dependencies]`). `rand` wird dafür von einer reinen
Dev-Abhängigkeit zu einer **optionalen** Abhängigkeit derselben Crate —
keine neue Kiste, nur eine andere Sichtbarkeit.

**Warum:** Dieses Repo ist öffentlich. Ein eingebetteter privater
Schlüssel, auch ein Wegwerfschlüssel, stünde für immer in der
Versionsgeschichte und würde jeden Leak-Scanner beschäftigen, der je darauf
schaut.

## 10. `KeyFileContent` bekommt **kein** `Debug` — auch kein geschwärztes

**Frage:** 5.1 verbietet ein **abgeleitetes** `Debug`. Ein handgeschriebenes,
schwärzendes `Debug` wäre erlaubt und bequemer (`expect_err` in Tests
verlangt `T: Debug`).

**Entscheidung:** gar kein `Debug`. Tests benutzen stattdessen
`.err().expect(…)`.

**Warum:** Ohne `Debug` verhindert der Compiler jeden versehentlichen
`{:?}`-Ausdruck, statt ihn zu schwärzen. Das ist während der Umsetzung
zweimal tatsächlich eingetreten (einmal in einem Test, einmal in einem
Kanal-`send`) und beide Male sofort aufgefallen — genau das soll es.

## 11. `~\` wird auf Windows aufgelöst (Festlegung, die die Spec offenlässt)

**Frage:** A-3 sagt „`~` am Anfang wird aufgelöst" ohne Plattformangabe und
nennt als Beispiel `~/.ssh/id_ed25519`. Ein Windows-Nutzer tippt
`~\.ssh\id_ed25519`.

**Entscheidung:** Auf Windows wird zusätzlich `~\` als Tilde-Präfix
erkannt. Auf Unix bleibt es bei `~/` — dort ist `\` ein gültiges
Dateinamenszeichen, und ein Pfad `~\foo` wäre eine Datei namens `~\foo`.

**Warum nicht anders:** Ohne den Zweig wäre `~\…` in den `~user`-Zweig
gefallen und mit „absoluter Pfad nötig" abgelehnt worden — eine stille
Verengung von A-3, und zwar genau auf der Plattform, auf der `\` die
normale Schreibweise ist.

**Was dabei nicht aufgeht:** Der Zweig öffnet keinen vorher geschlossenen
Weg. `is_absolute()` greift danach unverändert, `~user` verhält sich weiter
wie verlangt, und ein `~\..\..` erreicht nichts, was der Nutzer nicht schon
mit seinen eigenen Rechten lesen dürfte — §6.4.6 sieht den `..`-Fall
ausdrücklich vor.

**Ungetestet:** Für diesen Zweig gibt es keinen Test, weil er unter
`#[cfg(windows)]` liefe und die Umsetzung auf macOS entstanden ist. Die CI
testet auf `windows-latest`; ein `#[cfg(windows)]`-Pendant zu
`test_tilde_is_expanded_to_the_home_directory` gehört dort ergänzt.

## 12. Ein scheiternder Rollback bekommt einen **eigenen** Fehlercode

**Frage:** Bleibt nach einem gescheiterten Rollback ein privater Schlüssel
im Schlüsselbund liegen, muss der Nutzer es erfahren (C-6). Unter welchem
stabilen Code?

**Erste Fassung, und warum sie falsch war:** Zuerst stand dort
`KEYCHAIN_UNAVAILABLE`. Der zweite Review-Durchgang hat gezeigt, dass das
die Nachbesserung wirkungslos gemacht hätte: `errorCodes.ts` **ersetzt**
bei einem bekannten Code die Meldung durch den eigenen Übersetzungstext und
benutzt die mitgelieferte `message` gar nicht. Der Nutzer hätte „Der
Systemschlüsselbund ist nicht verfügbar" gelesen — und ausgerechnet nicht,
welcher Eintrag liegen geblieben ist. Dazu widerspräche es Spec 0071 A13:
Dieser Code gilt dort ausdrücklich nur, wenn der Schlüsselbund als nicht
verfügbar bekannt ist.

**Entscheidung:** eigener Code
`IDENTITY_FILE_ROLLBACK_LEFT_KEY_BEHIND` in `error.rs`. Solange Schritt 5
keine Übersetzung ergänzt, ist er dem Frontend unbekannt — und genau dann
zeigt es die `message`, die den Slot nennt. **Für Schritt 5:** Die
Übersetzung braucht den Slot als Parameter, sonst geht die Auskunft wieder
verloren.

Die ursprüngliche Fehlerursache (warum das Speichern scheiterte) bleibt im
Text erhalten und wird vom Rollback-Problem nicht verdrängt; ein Test
prüft beides.

## 13. Bewusst **nicht** behobene Review-Funde

Der `spec-reviewer` hat mit ERHÖHTER Priorität geprüft. Alle Funde bis auf
die folgenden sind behoben (Commit `Review-Nacharbeit`). Was stehen bleibt,
und warum:

**a) Der Wettlauf-Test (§6.4.7) ist probabilistisch.**
`test_permission_check_and_read_see_the_same_file` fährt 2 000 Durchläufe
gegen einen eng laufenden Tauscher. Gegen eine Pfad-`stat`-Umsetzung wird
er nur rot, wenn er das Zeitfenster trifft — beim Gegenbeweis geschah das
sofort, zugesichert ist es nicht.

*Warum nicht behoben:* Ein deterministischer Nachweis bräuchte einen Haken
mitten in `probe`, also Testinfrastruktur im Produktionspfad einer
sicherheitskritischen Funktion. Das ist der schlechtere Tausch. Die
Gegenrichtung ist sauber: Gegen die **richtige** Umsetzung kann der Test
nicht falsch-rot werden (`rename` ist atomar, der Pfad existiert
durchgehend). Falsch-negativ ja, falsch-positiv nein — und nur Letzteres
wäre gefährlich.

**b) `test_connection` trimmt die Passphrase nicht, `resolve_auth_method`
schon.** „Verbindung testen" und „Speichern" können für dieselbe Eingabe
unterschiedlich ausgehen.

*Warum nicht behoben:* Vorbestehend und nicht von dieser Spec verursacht —
der `PrivateKey`-Arm in `test_connection.rs` verhält sich seit jeher
genauso, sogar ohne die `trim()`-Leerprüfung, die der neue
`IdentityFile`-Arm hat. Das gehört in ein eigenes Item; es hier
mitzuerledigen hieße, eine fremde Klasse still in einem Spec-0076-Commit zu
ändern.

**c) Die Frontend-Typen kennen `identity_file` nicht.**
`apps/smart-ssh-community/frontend/src/types.ts:13`, dazu `ServerDto` ohne
`identityFilePath` und `ServerForm.tsx`.

*Warum nicht behoben:* Schritt 5, ausdrücklich nicht Teil dieses Laufs.
**Aber mit einer Warnung, die dort hingehört:** Heute ist der Zustand
unerreichbar, weil die Oberfläche keine solchen Server erzeugen kann. Mit
**Spec 0075** (Import) ändert sich das sofort — deren Import erzeugt
`IdentityFile`-Server. Wer 0075 vor Schritt 5 von 0076 fahren lässt,
bekommt Server, die die Oberfläche nicht darstellen kann. Diese Reihenfolge
gehört beachtet.

**d) `test_dev_stdin_yields_a_clean_outcome_and_never_hangs` ist in der CI
faktisch ein Rauchtest.** Mit stdin an `/dev/null` oder einer Datei kann er
für keinen Code-Defekt rot werden.

*Warum nicht behoben:* §6.4.6 nennt `/dev/stdin` namentlich, und die
Zusage, die dort steht („entweder ein sauberer Fehler oder ein gelesener
Schlüssel, in keinem Fall ein Hänger"), ist genau das, was der Test prüft.
Mehr gibt der Fall nicht her, weil `/dev/stdin` je nach Umgebung eine
reguläre Datei, ein Zeichengerät oder ein Rohr ist. Der belastbare
`O_NONBLOCK`-Nachweis ist der FIFO-Test, der belastbare Lesegrenzen-Nachweis
`test_an_endless_source_is_never_read_beyond_the_limit`.

**e) Der neue Abbruchpfad kann bei verfügbarem Schlüsselbund einen rohen
`keyring`-Text ans Frontend reichen.** `keychain_aware_credential_error`
hängt den stabilen Code nur an, wenn der Schlüsselbund schon beim Start als
nicht verfügbar erkannt wurde.

*Warum nicht behoben:* Das ist das bestehende Verhalten aller
Schlüsselbund-Schreibzugriffe in dieser Crate, und Spec 0071 X2 verspricht
die Unterdrückung ausdrücklich nur für den „nicht verfügbar"-Fall. Der Pfad
ist durch den Fix lediglich **neu erreichbar** (vorher verschluckte `.ok()`
den Fehler ganz — was schlechter war). Eine Änderung hier wäre eine
Spec-0071-Frage, keine Spec-0076-Frage.

**f) UNC-Pfade (`\\server\share\key`) gelten unter Windows als absolut.**
Ein Schlüssel würde über SMB gelesen.

*Warum nicht behoben:* Von dieser Spec weder eingeführt noch adressiert.
Gehört als eigenes Item festgehalten — die Frage ist eine Produktfrage
(„darf ein Schlüssel von einer Netzfreigabe kommen?"), keine Umsetzungsfrage.

## 14. Kein Changelog-Eintrag aus diesem Lauf

**Frage:** Der Abschluss eines Umsetzungsschritts sieht normalerweise ein
Fragment unter `changelog.d/` vor.

**Entscheidung:** Aus diesem Lauf kommt keines.

**Warum:** Nach den Schritten 1 bis 4 ist die Anmeldeart über die
Oberfläche **nicht erreichbar** — es gibt kein Formularfeld, keinen
Dateidialog, keinen Knopf. Ein Eintrag, der jetzt „Anmeldung mit einer
Schlüsseldatei" verspricht, wäre zum Zeitpunkt seines Erscheinens unwahr.
§7 legt den CHANGELOG ohnehin auf Schritt 6, also hinter das Frontend; dort
gehört auch das Fragment hin.

Die Tauri-Kommandos `inspect_key_file` und
`convert_identity_file_to_keychain` sind registriert und benutzbar — aber
nur von einem Frontend, das es noch nicht gibt.

**Nachtrag (Schritt 5, Commit `28b14fd`):** Mit dem Frontend ist die
Anmeldeart jetzt erreichbar; das Fragment
`changelog.d/0076-identity-file-auth.md` liegt bei.

## 15. Die Lösch-Vorschau behauptet für `identity_file` nicht mehr, ein Secret werde entfernt

**Frage:** `ServerForm.tsx`s Lösch-Vorschau (Spec 0046, Fund 1) zeigt für
jede Anmeldeart außer `agent` die Zeile „Wird aus dem Schlüsselbund
entfernt: `<Label der Anmeldeart>`" — für `password`/`privateKey`/
`certificate` korrekt, weil dort immer ein Pflicht-Secret im Schlüsselbund
liegt. Bei `identity_file` liegt dort höchstens eine **optionale**
Passphrase (`AuthMethod::IdentityFile.passphrase_ref`); ob sie gesetzt
ist, sagt `ServerDto` nicht (anders als `hasSudoPassword` für das
Sudo-Passwort). Ohne Eingriff hätte die Zeile für jeden
`identity_file`-Server „Wird aus dem Schlüsselbund entfernt: Schlüsseldatei"
behauptet — die Datei selbst liegt aber nie im Schlüsselbund und wird vom
Löschen nie berührt (C-5 gilt selbst hier sinngemäß: Löschen des Servers
ist keine Überführung, die Datei war nie dort).

**Entscheidung:** `identity_file` ist von der bedingungslosen Zeile
ausgenommen; stattdessen erscheint — unabhängig vom sonstigen Zustand —
ein eigener Text `identityFilePassphraseMayBeDeleted` („Falls für diese
Schlüsseldatei eine Passphrase hinterlegt ist, wird sie mit entfernt. Die
Schlüsseldatei selbst bleibt unangetastet auf der Platte liegen — sie
liegt nie im Schlüsselbund."). Das ist ehrlich in beide Richtungen: Weder
wird ein Secret behauptet, das vielleicht gar nicht existiert, noch wird
verschwiegen, dass eines existieren könnte — und anders als die
allgemeine „mag entfernt werden"-Formulierung (`secretMayBeDeleted`,
bisher nur für `sudoPasswordUnknown`, Spec 0071 A14) nennt der Text nicht
fälschlich „Systemschlüsselbund nicht lesbar" als Grund, wenn der
Schlüsselbund in Wahrheit einwandfrei lesbar ist, nur eben nicht sagen
kann, ob dort etwas für diesen Server liegt.
(spec-reviewer-Fund, Review dieses Schritts: die erste Fassung
wiederverwendete `secretMayBeDeleted` mit diesem irreführenden
Klammerzusatz.)

**Warum keine Rückfrage:** Kein Punkt aus Spec 0076 (die die Lösch-Vorschau
gar nicht erwähnt), sondern eine Ableitung aus dem bestehenden Prinzip
dieser Maske selbst und aus dem Produktgrundsatz „ehrliche Hinweise statt
Verharmlosung" (Coder-Skill, Abschnitt „Was bei diesem Produkt zählt" —
**nicht** Spec 0076, eine frühere Fassung dieses Absatzes zitierte fälschlich
„§6.3.2 der Spec" dafür, spec-reviewer-Fund) — sowie aus dem in derselben
Datei bereits vorhandenen Muster für „kann die Oberfläche nicht sicher
wissen" (`secretMayBeDeleted`). Eine neue `ServerDto`-Auskunft eigens dafür
(z. B. `hasIdentityFilePassphrase`, analog `hasSudoPassword`) wäre eine
größere, in der Spec nicht verlangte Änderung gewesen und ist unterblieben.
