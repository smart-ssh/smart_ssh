# ADR 0118 — Server mit unlesbarer Anmeldeart als eigener Listentyp

Status: akzeptiert
Betrifft: Spec 0008 (Abschnitt 6a), Spec 0101, Issue #100

## Kontext

Die Anmeldeart eines Servers steht als JSON in einer Textspalte. Kennt eine
Version eine dort gespeicherte Variante nicht (etwa nach dem Zurückgehen
von einer neueren Version) oder ist das JSON beschädigt, ließ eine einzige
solche Zeile das Laden **aller** Server scheitern: Serverbaum leer,
Secret-Umzug beim Start abgebrochen.

Issue #100 ließ die Form der Schnittstelle offen: (a) eine Liste, die
nicht nutzbare Einträge zusätzlich und getrennt liefert, `Server` bleibt
unverändert; (b) `Server.auth` optional machen.

## Entscheidungen

1. **Option (a), als zusätzliche Trait-Methode.** `ProfileStore` bekommt
   `list_server_entries` mit dem Ergebnis `ServerListing { servers,
   unusable }`. `list_servers` behält Signatur und Bedeutung „nutzbare
   Server" — im SQLite-Store lässt es nicht nutzbare Zeilen aus, statt zu
   scheitern. Damit überspringen alle bestehenden Aufrufer (Verbinden, MCP,
   SSH-Konfigurations-Export und -Import, Diagnose-Export, Gruppen-Löschen,
   Tag-Liste) den Eintrag automatisch, und die Zusage „ein `Server` hat
   immer eine nutzbare Anmeldeart" gilt weiter. Option (b) hätte jeden
   `match` auf die Anmeldeart und viele Test-Konstruktoren berührt und
   einen Rückfall auf eine Ersatz-Anmeldeart an jeder dieser Stellen
   möglich gemacht. Die Default-Implementierung liefert keine nicht
   nutzbaren Einträge; In-Memory-Stores brauchen keine Änderung.
2. **`get_server` scheitert sichtbar** für eine solche Zeile. Kein
   Ersatzwert; wer den Server direkt anfragt (Verbinden, Jump-Host,
   Bearbeiten, MCP), bekommt einen Fehler.
3. **Zwei Gründe:** gültiges JSON mit unbekanntem Inhalt (wahrscheinlich
   neuere Version) und nicht lesbares JSON (beschädigt). Behandelt werden
   beide gleich, nur der angezeigte Text unterscheidet sich.
4. **Löschen über einen eigenen Weg.** `delete_unusable_server` löscht die
   Zeile und alle Slots des festen Schemas `server:<id>:<slot>`, weil sich
   aus einer unlesbaren Anmeldeart nicht ableiten lässt, welche Slots
   belegt sind. Für einen nutzbaren Server lehnt dieser Weg ab — dort
   bestimmt das normale Löschen die Secrets aus der Anmeldeart. Ein Secret,
   das nicht entfernt werden konnte, steht wie beim normalen Löschen im
   Ergebnis (Spec 0071, A17).
5. **Der Secret-Umzug (Spec 0101) übernimmt die Secrets** eines nicht
   nutzbaren Servers über dieselben Slots, statt ihn zu überspringen. Beim
   Überspringen blieben seine Secrets im Schlüsselbund, während der Umzug
   als erledigt gilt — danach läse sie keine Version mehr. Der Umzug
   schreibt, liest zurück und löscht im Schlüsselbund erst nach dem
   Vergleich, wie für jedes andere Secret; die Zeile in `servers` wird nicht
   angefasst.
6. **Eigene Liste im Frontend** (`list_unusable_servers`) statt eines
   geänderten `list_servers`-Ergebnisses: Jeder bestehende Aufrufer im
   Frontend bekommt weiter nur verbindbare `ServerDto`s.

## Konsequenzen

- Slots, die erst eine neuere Version einführt, kennt diese Version nicht;
  sie bleiben beim Löschen und beim Umzug unberührt.
- Ein nicht nutzbarer Server fehlt in der Vorschau beim Löschen einer
  Gruppe (er landet wie jeder Server per `ON DELETE SET NULL` auf der
  Wurzelebene) und in der Konfliktprüfung des SSH-Konfigurations-Imports.
- Andere unlesbare Spalten (ID, Zeitstempel, Port) bleiben ein harter
  Fehler; sie entstehen nicht durch einen Versionswechsel.
