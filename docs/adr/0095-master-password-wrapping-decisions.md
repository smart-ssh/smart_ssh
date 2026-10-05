# ADR 0095 — Entscheidungen beim Verpacken des Wurzelschlüssels (Spec 0101, Etappe 3)

Status: angenommen · Spec: `docs/specs/0101-database-encryption.md` (A13–A19) ·
Backlog: BL-0203, BL-0314

Protokoll der Entscheidungen, die Spec 0101 für Etappe 3 offen gelassen hat
oder die vom Wortlaut abweichen. Die Begründungen stehen zusätzlich am Code;
hier sind sie an einer Stelle nachlesbar.

## 1. Eigenes Dateiformat statt PHC-String

A14 verlangt eine versionierte Datei mit Argon2id-Parametern, Salt, Nonce
und Chiffrat, „Kopf als AAD“. Umgesetzt als festes Binärformat
(`core/src/crypto/key_wrapping.rs`): Marke `SSHKWRP\0`, Formatversion,
KDF-Kennung, m/t/p als `u32` (little endian), Salt-Länge, Salt, Nonce,
Chiffrat samt Tag.

**Warum kein PHC-String:** Ein Format, das die Parameter aus der Datei
übernimmt, nimmt auch die Parameter eines Angreifers. Die Parameter müssen
in der Datei stehen, damit sie später erhöht werden können — sie werden
deshalb gegen eine Untergrenze geprüft **und** gehen als AAD in die
Authentifizierung ein.

## 2. Obergrenzen für die Argon2-Parameter (nicht in A14 gefordert)

m ≤ 1 GiB, t ≤ 16, p ≤ 16, geprüft **vor** der Ableitung.

**Grund:** Die Parameter stehen in einer Datei, die jemand mit Schreibzugriff
auf das Datenverzeichnis ändern kann. Ohne Obergrenze wäre `m = 4 GiB` eine
Speicherbombe, die vor dem ersten Fenster zuschlägt — und die Reihenfolge ist
nicht umkehrbar: Die Authentifizierung braucht den abgeleiteten Schlüssel.
Eine Grenze nach oben ist die einzige Stelle, an der dieser Fall abzufangen
ist. Sie liegt weit über den Schreibparametern, behindert also keine
Erhöhung. Der Reviewer hält fest, dass 1 GiB noch reichlich ist; das Senken
auf z. B. 256 MiB ist Härtung, kein Fehler, und **bleibt bewusst offen**
(Klarstellung 9 nimmt diesen Punkt ausdrücklich nicht auf).

## 3. Neuer Typ `RootKey` statt `Zeroizing<[u8; 32]>`

A19 verlangt Typen, die beim Freigeben überschrieben werden. `Zeroizing`
täte das, erbt aber das `Debug` seines Inhalts — ein `?key` in einer
`tracing`-Zeile hätte K als Byte-Liste ausgegeben. `RootKey` überschreibt
beim Freigeben **und** zeigt nichts.

**Nachgezogen (Klarstellung 9):** `RootKeyAccess::Unlocked` trägt jetzt
einen `RootKey`, nicht mehr ein `[u8; 32]`. `RootKey::take_from` überschreibt
das Array, aus dem es K nimmt — der Aufrufer gibt K also wirklich ab, statt
eine Kopie liegen zu lassen. Die Variante besitzt den Schlüssel (nicht
geliehen), weil der Moduswechsel aus D1 mitten in der Startschleife einen
neuen K einsetzt.

**Was von A19 (SOLL) offen bleibt:** Die Entscheidungstabelle kopiert K
weiter in ein `Option<[u8; 32]>`, weil der Bestand (`read_key_state`,
`DatabaseKey::from_root_key`, `AppState`s Chat-Cipher) mit `[u8; 32]`
arbeitet. Das umzustellen ist ein eigener Schritt und kein Teil dieser
Etappe. Ein Weg dieser Bytes in Log, DTO oder Diagnosepaket ist nicht
vorhanden (kein `Debug`, kein `Serialize`).

## 4. Abweichung von der Reihenfolge in A5 (Passwort-Modus)

A5 schreibt: „das neue Master-Passwort wird **zuerst** eingerichtet (A13),
die neue Verpackungsdatei geschrieben, erst dann werden die alten Dateien
umbenannt.“

Umgesetzt ist: Passwort abfragen → alte Dateien umbenennen → neue Verpackung
schreiben.

**Grund:** Beide Dateien tragen denselben Namen. Die neue Verpackung an ihren
Platz zu schreiben, *bevor* die alte weg ist, hieße sie zu überschreiben —
und A5 verlangt im selben Satz „löscht nichts“ und „nie überschreiben“. Der
abbrechbare Teil des Einrichtens (die Passworteingabe) liegt weiter vor jedem
`rename`: Bricht der Nutzer ab, ist nichts verändert. Die Zusicherung aus A5,
auf die es ankommt, bleibt damit erhalten; nur die Satzreihenfolge nicht.

**Folge, behoben (Klarstellung 9):** Scheitert das Schreiben der neuen
Verpackung nach den Umbenennungen, liegen die alten Dateien umbenannt da und
es gibt keine Verpackung. Dieser Pfad hat jetzt seinen eigenen Fehlerfall
(`MasterPasswordSetupFailedAfterRename`) mit einem Text, der sagt, wo die
Dateien liegen und dass sie zu behalten sind — statt der falschen Zusage
„es ist nichts verändert“.

## 5. `StartOverKey::Keep` — die Verpackungsdatei bleibt bei D2 stehen

A5 nennt die Verpackungsdatei im Umbenennungssatz als MUSS. Sie wird
trotzdem **nur** mitgenommen, wenn sie selbst das Unbrauchbare ist (D3,
`UnusableWrapping`).

**Grund:** D2 aus dem Feld *sonst* × *da* heißt „die Datei passt nicht zu
diesem Schlüssel“ — der Schlüssel ist in Ordnung. Die Verpackung dort
umzubenennen wäre der Verlust von K: Die Datenbank wird neu angelegt, und
ohne Verpackung gäbe es kein Passwort mehr, mit dem sie sich öffnen ließe.
Strenger als der Wortlaut wäre hier schädlich. Der Reviewer hat die
Begründung geprüft und keine schädliche Nebenwirkung gefunden.

## 6. Der Umzugs-Dialog ist eine eigene Dialogart

A11 sagt „Dialog D1 ohne Einrichten“. Umgesetzt als eigene Variante
`StartupDialog::MigrationUnreadable`, nicht als `D1 { offers_password_setup:
false }`. Dadurch kann dieser Dialog das Einrichten per Konstruktion nicht
anbieten (T13: „Umzugs-Dialog (A11) nie“), statt an einem `false` zu hängen,
das jemand später anders setzt. Die Texte sind dieselben.

## 7. Der Ausweg aus einer unbrauchbaren Verpackungsdatei (Klarstellung 9)

Eine Verpackungsdatei, die **kein** Passwort öffnet, führt in der Tabelle A3
nach *ungültig* und damit zu D3/D4. Der Weg dorthin ist
`start_over_from_unlock_screen` — ein **eigenes** Kommando in der
Positivliste des Tors, damit A16s dritte Wahl („Neu anfangen“) vor der
Entsperrung überhaupt erreichbar ist.

**Warum nicht ein Zweig des Entsperrens:** Entsperren und Neuanfangen sind
gegensätzlich — das eine holt K, das andere gibt ihn auf. In einem Aufruf
läge ein Tippfehler im Passwortfeld neben einem Codepfad, der Daten aufgibt.

**Der Riegel:** Angenommen wird der Aufruf nur, wenn
`wrapping_health(..).allows_starting_over()` gilt, also bei *unbrauchbar*
(Kopf, Version, KDF-Kennung, Parameter oder Chiffratlänge) oder *nicht
lesbar* (Rechte, hängende Verknüpfung). Bei einer brauchbaren Datei wird
abgelehnt: Sonst wäre der Ausweg bei bloß vergessenem Passwort ein Knopf,
der den Verlauf wegwirft, obwohl K noch zu holen ist. Die Oberfläche darf
fragen, nicht entscheiden.

`inspect_wrapped_key` und `unwrap_root_key` teilen dafür **dieselbe**
Funktion. Liefen die beiden Urteile getrennt, könnte eine Datei irgendwann
für das eine unbrauchbar und für das andere brauchbar sein — und der Nutzer
bekäme „Neu anfangen“ angeboten, obwohl sein K noch da ist.

## 8. A16 für Plugin-Kommandos: Registrierung statt Tor oder ACL

Gemessen (M7, `decision.md`): Der `invoke_handler` der App — dort sitzt
`StartupGate` — sieht **nur** die eigenen Kommandos. `plugin:store|get` und
Geschwister erreichen ihn nie und liefen vor der Entsperrung durch. A16
(„kein Kommando“) ist für sie deshalb anders geschlossen: Im
**Passwort-Modus** werden `dialog`, `opener`, `store`, `os` und
`notification` erst nach der Entsperrung registriert (gemessen M9:
`AppHandle::plugin` wirkt nach `build()`, und vorher ist das Kommando
unerreichbar — selbst wenn die ACL es erlaubt). Im Schlüsselbund-Modus
bleibt die Registrierung zur Bauzeit unverändert.

**Warum nicht die ACL** (`Manager::add_capability`, gemessen M8): Die
Capability-Datei gilt für beide Modi. Die Rechte dort zu entfernen und zur
Laufzeit nachzureichen hieße, im `setup()`-Haken gegen die nebenläufig
ladende Webview zu rennen — und die Sprachwahl (Spec 0024) ruft `store` und
`os` genau beim Start. Das Risiko läge damit auf dem Normalpfad, der
unverändert bleiben soll. Die Plugin-Registrierung wirkt nur im
Passwort-Modus und ist dort eine Verschärfung per Konstruktion.

`decoration` bleibt registriert: Es gestaltet den Fensterrahmen und liest
keine Datei, keine Einstellung und keinen Zustand des Betriebssystems.

**Folge für Commit 11:** Vor der Entsperrung ist die gespeicherte Sprachwahl
aus Spec 0024 nicht lesbar — Klarstellung 9 verlangt genau das. Die
Entsperrmaske nimmt die Sprache, die das Backend aus der Umgebung bestimmt
hat (Spec 0071 A11a/A11b: **eine** Sprachwahl für alle Startdialoge eines
Programmlaufs); sie steht dafür in `StartupStateDto`.

## 9. Zeitgrenze für einen Startdialog: fünf Minuten

`emit` liefert `Ok`, auch wenn niemand zuhört. Ohne Grenze war der gesperrte
Zustand bei einem verlorenen Ereignis ein Dauerhänger. Die Grenze ist
bewusst großzügig: Hinter der Frage steht ein Mensch, der ein Passwort
eintippt. Eine kurze Grenze mit einer Rückmeldung aus der Oberfläche wäre
genauer, bräuchte aber ein zusätzliches Kommando — eine Zusage, die die
Maske aus Commit 11 erst einlösen müsste; scheiterte sie daran, liefe jeder
Dialog in die Grenze.

Ein Zeitablauf ist **nicht** dasselbe wie ein Abbruch: Der Fragesteller
kennzeichnet ihn, damit die Meldung nicht „abgebrochen“ sagt, wo der Nutzer
nichts gesehen hat.

## 10. Bewusst nicht behoben in diesem Schritt

- **Commit 11 (Oberfläche) fehlt** — Budget, nicht Fachlichkeit. Der dritte
  D1-Knopf („Master-Passwort einrichten“) wird deshalb nativ noch nicht
  gezeigt: Der Weg dahinter ist fertig, nur die Maske fehlt, und ohne sie
  stünde der Nutzer vor einem Fenster ohne Eingabefeld.
- **Obergrenzen der Argon2-Parameter** bleiben bei 1 GiB / t 16 / p 16
  (Punkt 2). Härtung, kein Fehler; Klarstellung 9 nimmt den Punkt nicht auf.
- **A19 bleibt im Bestand unerfüllt**, soweit `read_key_state` und
  `DatabaseKey::from_root_key` mit `[u8; 32]` arbeiten (Punkt 3).
- **T17 und die zweite Hälfte von T18 waren nicht als eigene Tests belegt.**
  T17 war durch die vorhandenen Prüfungen auf Fehlertexte und die
  Log-Positivliste abgedeckt, T18s zweite Hälfte (MCP-Anfrage im gesperrten
  Zustand) strukturell — der MCP-Server startet erst nach `manage`. Ein Test,
  der das **zeigt**, fehlte. **Nachgezogen:** `master_password::tests::
  test_t17_…` und das Modul `app_shell::startup_gate_wiring`; wie und mit
  welchem Preis, steht in ADR 0096.
