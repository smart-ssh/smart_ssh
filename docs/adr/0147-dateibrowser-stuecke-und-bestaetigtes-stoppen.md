# ADR 0147 — Dateibrowser: Übertragungen in Stücken, Bestätigung vor dem Stoppen erhöhter Operationen

Status: angenommen · Issue: #622 · Spec 0085 A1.3, Spec 0086 A5

## Kontext

Ein Download las die ganze Datei in den Speicher, Übertragungen meldeten nur Start und Ende, und das Ausschalten des erhöhten Modus ließ laufende Übertragungen unbemerkt weiterlaufen, während rekursives Löschen und chmod sofort stoppten.

## Entscheidungen

1. **Stücke über den Transport.** `SftpSession` bekommt `read_file_chunked` und `write_file_chunked` mit einem Rückruf je Stück (256 KiB). Der Rückruf kann stoppen; geprüft wird vor jedem Stück, danach wird nichts mehr verarbeitet.
2. **Register laufender Operationen je Sitzung** in der Registry des erhöhten Kanals: Art, Pfad, Fortschritt, Stopp-Marke. Jede Stelle, die den Kanal widerruft, kommt dort vorbei. Rekursives Löschen und chmod haben keine Zeile in der Übertragungsliste und erscheinen nur im Dialog.
3. **Erst fragen, dann widerrufen.** Ausschalten und Nutzerwechsel geben bei laufenden Operationen die Liste zurück und ändern nichts; mit Bestätigung werden die Operationen gestoppt, ihr Aufräumen (Temp-Datei, Teil-Upload) läuft noch über den gültigen Kanal, höchstens 10 s, danach wird der Kanal in jedem Fall widerrufen (Sicherheit vor Aufräumen). Trennen und Neu-Aktivieren ohne Dialog behalten das bisherige Verhalten: Zeile mit dem Text „nicht mehr aktiv“.
4. **Ordner sind eine Operation.** Ein Ordner-Transfer ist ein Eintrag im Register; der Abbrechen-Knopf der laufenden Datei stoppt den ganzen Ordner. Die Zeilen je Datei bleiben, die Fortschrittsereignisse tragen die Summen.
5. **Downloads über Temp-Datei, Uploads direkt.** Ein Upload schreibt weiter unter dem Zielnamen (kein Umbenennen auf dem Server, um die Rechte und Ersetzungssemantik des Servers nicht zu ändern); bei Stopp wird die Teildatei entfernt, sonst nennt die Zeile ihren Pfad.

## Konsequenzen

- Der Speicher je Übertragung ist durch die Stückgröße begrenzt.
- Das Ausschalten kann bis zu 10 s dauern, wenn eine Operation nicht auf den Stopp reagiert.
- Ein abgebrochener Upload kann bei einem Verbindungsproblem eine Teildatei hinterlassen; die Zeile benennt sie.
