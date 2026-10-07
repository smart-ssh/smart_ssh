# ADR 0112 — Rückbau der feldweisen Verschlüsselung

Status: akzeptiert
Betrifft: Spec 0036, Spec 0101 (E11), Spec 0040 §3, Spec 0057 §1.3, Issue #113

## Problem

Chat-Nachrichten, Einträge des Ausführungsprotokolls (Ledger),
Eingabe-Historie und Zusammenfassungen lagen je Zeile unter
ChaCha20-Poly1305 mit dem Wurzelschlüssel K in der Datenbank (Spec 0036).
Seit Spec 0101 ist die ganze Datei mit einem aus demselben K abgeleiteten
Schlüssel verschlüsselt. Die zweite Schicht schützt nichts zusätzlich, kostet
aber Code-Pfade und Sonderfälle (Stores als `Option`, eine eigene
Alt-Klartext-Migration der Eingabe-Historie, ein Cipher-Parameter je Store).

Issue #113 verlangt den Rückbau, eine absturzsichere Umstellung der
vorhandenen Zeilen und hat entschieden: Zeilen, die mit dem aktuellen K nicht
lesbar sind, werden entfernt, und der Nutzer erfährt einmal, wie viele.
Offen ließ es die Form der Spalten und mehrere Einzelheiten der Umstellung.

## Entscheidung

1. **Klartext als SQLite-`TEXT` in den bestehenden `BLOB`-Spalten, kein
   Tabellen-Umbau.** SQLite speichert einen Wert in einer als `BLOB`
   deklarierten Spalte unverändert mit seiner eigenen Speicherklasse. Ein
   Umbau auf `TEXT` hieße, drei Tabellen neu anzulegen und umzukopieren
   (samt Fremdschlüsseln und Indizes) und für `chat_sessions` eine vierte —
   mehr Migrationsrisiko für eine rein kosmetische Deklaration. Die
   Speicherklasse wird stattdessen zum Merkmal: Die Stores schrieben bisher
   ausschließlich Blobs und schreiben jetzt ausschließlich Text.
2. **Altzeile = Wert mit Speicherklasse `BLOB`.** Text bleibt unberührt. Das
   deckt auch die Eingabe-Historie aus der Zeit vor Spec 0040 ab, die nie
   verschlüsselt wurde (Migration 0010 hat ihren Text unverändert kopiert);
   die frühere Alt-Klartext-Migration entfällt dadurch.
3. **Eine Transaktion plus Zustandstabelle.** Migration 0018 legt
   `field_content_decryption_state` (`open`/`done`) an. Beim Start läuft die
   Umstellung, solange der Zustand `open` ist, in genau einer Transaktion,
   die am Ende auch `done` setzt. Ein Abbruch rollt alles zurück; der
   nächste Start beginnt von vorn. Die Altzeilen werden in Portionen
   gelesen, damit lange Verläufe den Speicher nicht sprengen; die
   Transaktion wird dafür nicht geteilt.
4. **Nicht lesbare Zeilen:** Nachricht, Ledger-Eintrag und Historie-Eintrag
   werden als Zeile gelöscht. Bei der Zusammenfassung wird nur sie (Text und
   Rundenzahl) geleert — die Sitzung mit ihren Nachrichten bleibt; sie zu
   löschen hätte lesbare Daten mitgerissen. Gezählt wird beides als
   „entfernter Eintrag". Das Ledger bleibt im Betrieb append-only; die
   Umstellung ist der einzige Weg, der einzelne Einträge entfernt, und nur
   solche, die mit K ohnehin nicht mehr lesbar waren.
5. **Hinweis höchstens einmal.** Er erscheint nach dem Festschreiben, wenn
   mindestens ein Eintrag entfernt wurde; danach steht der Zustand auf
   `done`, und die Umstellung läuft nie wieder. Bricht die App genau
   zwischen Festschreiben und Hinweis ab, entfällt er — gewählt wurde
   „höchstens einmal" statt „mindestens einmal", weil ein wiederholter
   Hinweis über längst Entferntes irreführender wäre als ein fehlender.
6. **Scheitern endet sichtbar.** Ein Fehler der Umstellung beendet den Start
   mit einem eigenen Fehlerfall (`FieldContentDecryptionFailed`), dessen
   Text sagt, dass nichts verändert wurde, und — anders als der allgemeine
   Fall — nicht zu einem Backup rät: Die Datenbank hat sich gerade öffnen
   lassen. Kein Weiterlauf mit Spalten, die kein Store mehr lesen kann.
7. **Entschlüsseln bleibt, eng begrenzt.** Das Lesen der Altzeilen liegt in
   `core::crypto::legacy_field_content` und hat genau einen Produktiv-
   Aufrufer, die Umstellung. Den Schreibweg gibt es nur für Tests (hinter
   `test-support`). `chacha20poly1305` bleibt Abhängigkeit, weil die
   Verpackung von K unter einem Master-Passwort (Spec 0101, A14) es nutzt.
8. **`Option` entfällt nur, wo es „kein Schlüssel" hieß.** Die Stores im
   `AppState` sind jetzt Pflichtfelder. An der einzelnen Sitzung bleiben sie
   `Option`: Dort bedeutet `None` „diese Sitzung hat keine eigene
   `chat_sessions`-Zeile" (lokaler Server ohne Persistenz, Tests). Ebenso
   bleibt der optionale Historie-Parameter der Chat-Versandfunktion als
   Testnaht; die App übergibt immer den Store.

## Konsequenzen

- K bleibt die Wurzel des Datenbankschlüssels; Schlüsselbund,
  Master-Passwort und Ableitung sind unverändert.
- Ein Build mit dieser Änderung hinterlässt Schema-Version 18. Ein älterer
  Build lehnt die Datei deshalb mit „neuere Version" ab (Spec 0059), statt
  Klartext als Chiffrat lesen zu wollen.
- Die Rohdatei-Nachweise (Spec 0096) prüfen jetzt die verschlüsselte Datei;
  ihre Gegenprobe ist eine Klartext-Datei statt eines durchreichenden
  Ciphers.
