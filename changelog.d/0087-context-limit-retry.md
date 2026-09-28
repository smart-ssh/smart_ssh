### Behoben
- Ein selbstgehosteter KI-Server mit kleinem Kontextfenster lieferte bei zu
  großem Antwortbudget bisher einen Fehler statt einer Antwort. Die App
  erkennt diesen Fall jetzt an der Fehlermeldung des Servers und wiederholt
  die Anfrage einmal mit einem kleineren Budget.
- Die Fehlermeldung „Kontext zu groß für den KI-Provider“ nennt jetzt die
  Abhilfe: die Einstellung „Max. Antwortlänge (Tokens)“ niedriger setzen
  oder einen neuen Chat beginnen.
- Bei Anthropic unterschreitet der automatische Wiederholungsversuch nach
  einer abgeschnittenen Antwort nicht mehr eine selbst eingestellte „Max.
  Antwortlänge“.
