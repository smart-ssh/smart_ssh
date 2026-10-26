# ADR 0139 — Erweiterungseigene Datenbankmigrationen

## Status

Accepted · Issue: #462 · Spec 0038 §2a

## Kontext

Erweiterungen sollen eigene Tabellen in der verschlüsselten Anwendungsdatenbank führen (Spec 0038 §2a). Das Issue lässt offen, ob `sqlx::migrate::Migrator` zur Laufzeit mit eigenem Verlaufstabellennamen reicht oder ob ein eigener Runner nötig ist, und wie „nur Tabellen mit Präfix“ durchgesetzt wird.

## Entscheidungen

1. **Eigener kleiner Runner (Option b).** `Migrator` führt den Verlauf in `_sqlx_migrations`; in sqlx 0.9 ist der Tabellenname nicht einstellbar. Der Runner legt je Erweiterung `<präfix>schema_migrations` an (im Namensraum der Erweiterung, deshalb unter Kontrolle derselben Regel) — erst beim ersten Lauf, damit eine Datenbank ohne Erweiterung unverändert bleibt. Er führt jede Migration in einer eigenen Transaktion samt Verlaufszeile aus und prüft eine Prüfsumme (FNV-1a über den SQL-Text; sie erkennt versehentliche Änderungen, ist kein Sicherheitsmerkmal).
2. **Durchsetzung über den SQLite-Autorisierer** statt SQL-Parsing. Während der Migration hängt `sqlite3_set_authorizer` an der Verbindung und weist alles ab, was nicht dem Präfix gehört (auch Lesen), dazu `PRAGMA`, `ATTACH`, Sichten, Trigger, virtuelle Tabellen, Transaktionssteuerung und `load_extension`. Ein Parser würde Randfälle des SQL-Dialekts nachbauen; der Autorisierer sieht genau die Operationen, die SQLite ausführt. Schreiben auf `sqlite_master` ist zugelassen, weil SQLite damit CREATE/ALTER/DROP selbst bucht; ein handgeschriebener Zugriff braucht `PRAGMA writable_schema`, das abgewiesen wird. Fremdschlüssel in den Kern sind kein Autorisierer-Ereignis und werden nach der Migration geprüft. Der Preis ist ein `unsafe`-Block (FFI auf `libsqlite3-sys`, schon Abhängigkeit); die Verbindung wird bei Abbruch des Futures geschlossen, damit kein Autorisierer im Pool zurückbleibt.
3. **Das Handle ist der Pool, kein Sandkasten.** Erweiterungscode ist Teil des Binaries. Die Prüfung schützt das Kernschema vor einer fehlerhaften Migration, nicht vor späteren Abfragen der Erweiterung.
4. **Fehler tragen keinen SQL-Text.** `ExtensionMigrationError` enthält Version, Kategorie bzw. den SQLite-Ergebniscode; das Protokoll nennt zusätzlich das Präfix.
5. **Ort des Aufrufs.** `open_and_assemble` ist der einzige Ort, der erst existiert, wenn die Datenbank offen ist (Schlüsselbund-Modus: beim Start, Passwort-Modus: nach dem Entsperren). Ein scheiternde Erweiterung wird übersprungen, der Kern startet.

## Konsequenzen

- Tests für das Voll-Backup können erst mit dessen Umsetzung den Ausschluss gegen echten Exportcode prüfen; bis dahin steht die Zusage in Spec 0109 C2.
- Ein Präfix darf kein Unterstrich-Segment enthalten; so kann ein Präfix nie den Namensraum eines anderen enthalten.
