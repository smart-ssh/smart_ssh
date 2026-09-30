### Changed

- Die Logdatei enthält auf dem Standard-Level keine Inhalte mehr: kein
  Kommando, keine Kommando-Ausgabe, keinen Chat-, Notiz- oder Prompt-Text,
  keine Werkzeug-Argumente und keine Fehlermeldung einer KI- oder
  SSH-Verbindung. Stattdessen stehen dort nur noch inhaltsfreie Angaben —
  IDs, Entscheidung, gegriffene Regel, Längen, Exit-Code, Fehlercodes. Das
  gilt unabhängig davon, ob ein Passwort in einer Form geschrieben ist, die
  die automatische Unterdrückung erkennt. Wer die Inhalte zur Fehlersuche
  braucht, startet die App mit `RUST_LOG=debug`; dort laufen sie weiter
  durch die Unterdrückung. Einzige Ausnahme bleibt die Antwort eines
  KI-Anbieters auf einen Fehler — sie wird gebraucht, um eine
  Fehlkonfiguration zu erkennen, und steht unterdrückt und auf 512 Zeichen
  gekürzt im Log.
