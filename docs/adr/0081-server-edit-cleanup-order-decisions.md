# 0081-server-edit-cleanup-order-decisions

## Status
Accepted

## Kontext

Spec 0082 dreht die Reihenfolge beim Bearbeiten eines Servers um: Die
Schlüsselbund-Einträge der bisherigen Anmeldeart werden erst entfernt,
nachdem die Datenbank die neue trägt. Vorher räumte `resolve_auth_method`
als Erstes auf — scheiterte danach irgendetwas, stand in der Datenbank
weiter die alte Anmeldeart, deren Secrets es nicht mehr gab.

Dieses ADR hält die Entscheidungen fest, die die Spec offen ließ, sowie die
Funde des spec-reviewers, die bewusst nicht behoben wurden.

## Entscheidung 1: Der Rückweg kennt die geschriebenen Refs über eine Hülle, nicht über einen Rückgabewert

A3 verlangt, dass ein gescheitertes Bearbeiten die Einträge zurücknimmt,
die **dieser Aufruf** geschrieben hat. Dafür muss der Rückweg sie kennen.
Zwei Wege standen zur Wahl:

1. `resolve_auth_method` (und `resolve_sudo_password`) geben zusätzlich eine
   Liste der geschriebenen Refs zurück.
2. Der Aufruf läuft über eine `CredentialStore`-Hülle, die jedes `set`
   mitschreibt.

**Gewählt: (2)**, `RecordingCredentialStore`. Bei (1) hängt die
Vollständigkeit der Liste daran, dass jeder künftige Zweig sie mitpflegt —
eine übersehene Schreibstelle hieße ein verwaister Eintrag, von dem niemand
erfährt, und kein Test fiele darauf. Bei (2) hängt die Aufzeichnung am `set`
selbst; ein neuer Slot ist automatisch erfasst. Für eine Zusage über
Credentials ist der Unterschied zwischen „gilt, solange alle aufpassen" und
„gilt per Konstruktion" der ganze Punkt.

Die Hülle vermerkt einen Ref auch dann, wenn das `set` **fehlschlägt**. Der
Rückweg darf sich nicht darauf verlassen, dass ein Schlüsselbund im
Fehlerfall garantiert nichts hinterlassen hat. Ein Ref zu viel kostet ein
Löschen ins Leere (`NotFound` gilt als Erfolg); ein Ref zu wenig hieße, einen
verwaisten Eintrag stehen zu lassen.

Sie ist `pub(crate)` und wird genau einmal konstruiert, in
`servers::update_server`. Länger gehalten — etwa im `AppState` — wüchse
`written` über alle Speichervorgänge hinweg, und ein späterer Rückweg
löschte Einträge eines früheren, längst erfolgreichen Aufrufs.

## Entscheidung 2: Mengendifferenz statt Paarliste

§5 verlangt, dass es die Zuordnung „welche Refs gehören zu einer
`AuthMethod`" nur noch einmal gibt. Umgesetzt als
`auth_method_credential_refs`; aufgeräumt wird die Differenz „Refs der
bisherigen Art minus Refs der gespeicherten".

Das ersetzt eine Paarliste, die per `matches!` prüfte, ob alte und neue
Anmeldeart dieselbe Art sind. Deren impliziter `false`-Zweig war schon
einmal ein Credential-Verlust (Spec 0076, §7.1: das fehlende Paar
`(IdentityFile, IdentityFile)` löschte beim bloßen Bearbeiten die
hinterlegte Passphrase). Die Differenz hat keinen solchen Zweig: „gleiche
Art" ist der Sonderfall „leere Differenz", ein von zwei Arten geteilter Slot
steht auf beiden Seiten und kann nicht wegfallen, und eine neue
`AuthMethod`-Variante erzwingt einen Compilerfehler statt eines stillen
Fehlverhaltens.

## Entscheidung 3: Der Rückweg hängt an der Struktur, nicht an Aufmerksamkeit

Zunächst stand der Rückweg dreimal im Code, einmal je fehlbarem Schritt.
Der spec-reviewer hielt das für tragfähig, aber für die einzige Stelle des
Entwurfs, die nicht strukturell abgesichert ist: Ein künftiges `?` zwischen
Schlüsselbund und Datenbank umginge ihn lautlos.

**Gewählt:** Der schreibende Teil steckt in `write_edited_server`, einer
eigenen `async fn`, die `?` benutzen darf und selbst nichts aufräumt.
`update_server` hat genau einen Erfolgs- und einen Fehlerweg am `match` über
deren Ergebnis. Der Reviewer hatte das als zurückstellbar eingestuft;
umgesetzt wurde es trotzdem, weil ein lautlos umgehbarer Rückweg
ausgerechnet im Fix gegen Credential-Verlust die falsche Hinterlassenschaft
wäre. Die zweite Runde hat bestätigt, dass sich dabei kein Pfad verändert
hat.

Die vier Prüfungen **vor** der Hülle (`is_local`, `reject_local_jump_host`,
`normalize_sftp_server_path`, `get_server`) benutzen weiter `?`. Das ist
gefahrlos: Bis dorthin hat der Aufruf nichts geschrieben, es gibt nichts
zurückzunehmen.

## Entscheidung 4: `sudo_password` ist auf **beiden** Aufräumwegen ausgenommen

A3 nennt den Slot ausdrücklich für den Rückweg. A2/A5 nennen ihn nicht — sie
setzen stillschweigend voraus, dass er nicht zu einer `AuthMethod` gehört.
Unter dieser Annahme sind die Mengen disjunkt und die Ausnahme ein No-op.

**Gewählt:** Die Ausnahme steht trotzdem auf beiden Wegen, damit die Annahme
nicht bloß gilt, sondern geprüft wird. Verweist eine gespeicherte
`AuthMethod` doch einmal auf `server:{id}:sudo_password` — eine von Hand
veränderte Zeile, eine künftige Variante mit demselben Slot-Namen —, löschte
der Erfolgsweg sonst das Sudo-Passwort, das derselbe Aufruf gerade
geschrieben hat.

Die Kehrseite ist bekannt und gewollt: In eben diesem Zustand bleibt ohne
neues Sudo-Passwort ein verwaister Eintrag stehen, statt geleert zu werden.
Ein Eintrag zu viel ist der bessere Tausch als ein Credential zu wenig.

Damit ist der Code an dieser Stelle strenger als der Wortlaut von A5. Als
Klarstellung K1 in §9 der Spec festgehalten, nicht als stille Abweichung.

## Bewusst nicht behoben

- **R1 (aus der Spec, §8).** Nutzen alte und neue Anmeldeart denselben Ref,
  überschreibt der Aufruf den alten Wert vor dem Datenbank-Schreiben;
  scheitert danach etwas, bleibt der neue Wert stehen. Zurücksetzen hieße,
  bei jedem Speichern erst den alten Wert auszulesen.
- **R2 (aus der Spec, §8).** Zwei gleichzeitige Speichervorgänge desselben
  Servers rechnen je mit ihrem Anfangsstand.
- **`write_edited_server` nimmt `&(dyn CredentialStore + Send + Sync)`, nicht
  `&RecordingCredentialStore<'_>`** (spec-reviewer, Runde 2, als
  zurückstellbar eingestuft). Am einzigen Aufrufort sind die Hülle und der
  rohe Store beide in Reichweite und zuweisungskompatibel; wer versehentlich
  den rohen Store übergäbe, schaltete den Rückweg lautlos ab. Gefangen würde
  das heute von T5/T17/T17b (der verwaiste Eintrag bliebe stehen) — ein
  offener Defekt ist es also nicht, aber der Compiler könnte es früher
  fangen. Nicht in diesem Schritt umgesetzt, damit die Runde-2-Freigabe für
  den abgegebenen Stand gilt.
- **Abbruch der Tauri-Task zwischen Schlüsselbund- und Datenbank-Schreiben.**
  Wird die Future gedroppt, läuft kein Rückweg, und ein frisch geschriebener
  Eintrag bleibt verwaist. Vor und nach diesem Schritt gleich, also kein
  Regress; bisher nirgends festgehalten, deshalb hier.
- **Changelog-Kategorie `### Behoben` statt `### Sicherheit`** (spec-reviewer,
  Runde 1, als Ermessensfrage gestellt). Keep a Changelog führt „Security"
  für Verwundbarkeiten; die vergleichbaren Fragmente im Ordner härten je eine
  Schutzschicht. Hier geht es um Datenverlust beim Bearbeiten, keinen Weg für
  Angreifer.

## Folgen

Der Bearbeiten-Ablauf ist erstmals ohne Tauri-Laufzeit testbar (A7): 22
Tests fahren ihn als Ganzes. `reject_local_jump_host` zog dafür von
`app-shell` nach `app-logic`; `commands::update_server` reicht nur noch
durch. `create_server` behält seinen eigenen, vollständigen Rückweg — dort
ist die `ServerId` frisch, unter ihr kann nichts Legitimes stehen, und alle
Slots dürfen abgeräumt werden. Genau das darf beim Bearbeiten nicht
passieren; die beiden Rückwege bleiben deshalb getrennt.
