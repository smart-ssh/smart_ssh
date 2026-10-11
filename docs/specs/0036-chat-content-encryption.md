# Spec 0036 — Schutz persistierter Chat-Inhalte

Status: umgesetzt
Zweck: Beschreibt, wie Chat-Inhalte auf der Platte geschützt sind (durch die Verschlüsselung der ganzen Datenbank) und wie vorhandene, früher feldweise verschlüsselte Daten umgestellt werden.
Bezüge: Spec 0034 (Chat-Sitzungen), Spec 0101 (Datenbankverschlüsselung), Spec 0096 (Rohdatei-Nachweis).

## 1. Was geschützt ist

Diese Inhalte liegen dauerhaft in der Datenbank:

- Chat-Nachrichten einer Sitzung (Spec 0034),
- das Ausführungsprotokoll einer Sitzung (Spec 0057),
- die Eingabe-Historie je Server (Spec 0015),
- die rollierende Zusammenfassung einer Sitzung (Spec 0057).

Sie sind durch die Verschlüsselung der **ganzen Datenbankdatei** geschützt
(Spec 0101). Weder in der Datenbankdatei noch in ihren Begleitdateien steht
einer dieser Inhalte im Klartext; ohne den Wurzelschlüssel K lässt sich die
Datei nicht öffnen. Eine zusätzliche, eigene Verschlüsselung je Feld gibt es
nicht mehr.

Metadaten (Servernamen, Zeitstempel, Sitzungstitel, Regelkonfiguration)
liegen ebenso in der verschlüsselten Datei; für sie gilt dasselbe.

## 2. Frühere feldweise Verschlüsselung

Bis Issue #113 lagen die vier Inhalte aus Abschnitt 1 zusätzlich je Eintrag
unter ChaCha20-Poly1305 mit K in der Datenbank. Diese Schicht ist
zurückgenommen (Issue #113), weil die Datei seit Spec 0101 mit einem aus demselben K
abgeleiteten Schlüssel verschlüsselt ist und die zweite Schicht nichts
zusätzlich schützte. K bleibt die Wurzel des Datenbankschlüssels; an
Schlüsselbund, Master-Passwort und Schlüsselableitung ändert sich nichts.

## 3. Umstellung vorhandener Daten

- **U1** Beim ersten Start einer Version mit dieser Änderung werden alle noch
  feldweise verschlüsselten Einträge mit dem aktuellen K entschlüsselt und
  als Klartext in der verschlüsselten Datei gespeichert. Danach liest und
  schreibt die App diese Inhalte nur noch so. Im Verlauf sieht der Nutzer
  dieselben Inhalte wie vorher.
- **U2** Die Umstellung läuft genau einmal. Spätere Starts fassen nichts mehr
  an.
- **U3** Die Umstellung ist alles oder nichts. Wird sie unterbrochen
  (Absturz, Plattenfehler), bleibt jeder Eintrag im alten Stand, und der
  nächste Start führt sie vollständig durch. Es geht dabei nichts verloren.
- **U4** Ein Eintrag, der sich mit dem aktuellen K nicht entschlüsseln lässt
  (z. B. weil der Schlüssel nach Spec 0101 D4 neu erzeugt wurde), wird
  entfernt: eine Chat-Nachricht, ein Protokolleintrag oder ein
  Historie-Eintrag als Ganzes, bei einer Zusammenfassung nur die
  Zusammenfassung — die Sitzung mit ihren lesbaren Nachrichten bleibt.
  Nichts bleibt halb lesbar zurück. Für alle vier Inhalte gilt dieselbe
  Regel.
- **U5** Wurde mindestens ein Eintrag entfernt, sieht der Nutzer nach der
  Umstellung **einmal** einen Hinweis mit der Anzahl der entfernten alten
  Einträge und dem Satz, dass alles andere erhalten ist. Wurde nichts
  entfernt, erscheint kein Hinweis.
- **U6** Scheitert die Umstellung, startet die App nicht weiter, sondern
  zeigt eine eigene Fehlermeldung: Die Datenbank ließ sich öffnen, es wurde
  nichts verändert, nächster Schritt ist ein erneuter Start nach Prüfung von
  Schreibrechten und Speicherplatz; die Meldung rät nicht zu einem Backup.
- **U7** Einträge der Eingabe-Historie aus der Zeit vor ihrer Verschlüsselung
  (Spec 0040), die nie verschlüsselt wurden, bleiben unverändert erhalten.

## 4. Grenzen

- Ältere Versionen der App öffnen eine umgestellte Datenbank nicht; sie
  melden, dass die Datenbank von einer neueren Version stammt (Spec 0059).
  Ohnehin können Versionen vor Spec 0101 die verschlüsselte Datei nicht
  öffnen.
- Ein entfernter Eintrag (U4) lässt sich nicht wiederherstellen; er war
  schon vor der Umstellung mit dem vorhandenen Schlüssel nicht mehr lesbar.
