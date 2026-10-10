# Spec 0046 — Kleinere Korrekturen: Server löschen, Regel-Test, Diff-Vorschau, wartende Aktionen, Port-Anzeige

Status: umgesetzt
Zweck: Fünf unabhängige, kleine Verhaltensregeln, die zusammen erfasst sind, weil keine eine eigene Spec trägt: zweistufiges Löschen eines Servers, Tags im Regel-Test-Panel, Größengrenze der Diff-Vorschau, Ablauf wartender Bestätigungen und die Port-Anzeige des lokalen Pseudo-Servers.
Bezüge: Spec 0008 (Server-Verwaltung), Spec 0009 (Regel-Test-Panel, Abschnitt 6), Spec 0019 (Notiz-Diff), Spec 0020 (Dateikanal, Abschnitt 4.2), Spec 0017 (Tabs, Abschnitt 5), Spec 0007 (Bestätigung), Spec 0032 (lokaler Pseudo-Server), Spec 0071 (A17: Rückstände im Schlüsselbund).

## Fund 1 — Server löschen mit Vorschau und Bestätigung

Das Löschen eines Servers ist zweistufig, wie das Löschen einer Gruppe
(Spec 0008, Abschnitt 3). Der erste Schritt löscht nichts, sondern zeigt die
Auswirkungen:

- welche Secrets aus dem Schlüsselbund entfernt werden (je nach Anmeldeart das
  Server-Secret, ein hinterlegtes Sudo-Passwort); ist nicht sicher, ob ein Secret
  existiert (Schlüsselbund nicht lesbar, optionale Passphrase einer Schlüsseldatei),
  steht dort „kann gelöscht werden" statt „wird gelöscht";
- welche anderen Server ihre Jump-Host-Referenz auf diesen Server verlieren;
- immer, dass Chat-Historie, Notizen und Tags dieses Servers unwiderruflich
  mitgelöscht werden.

Erst ein bestätigender zweiter Schritt löscht. Ohne Bestätigung bleibt alles
unverändert. Die Reihenfolge beim Löschen bleibt: erst Schlüsselbund, dann
Datenbank. Konnte ein Secret nicht entfernt werden, ist der Server trotzdem
gelöscht, und die Oberfläche nennt die verwaisten Einträge (Spec 0071, A17).
Der lokale Pseudo-Server lässt sich nicht löschen.

## Fund 2 — Regel-Test-Panel übernimmt die Tags des gewählten Servers

Wählt der Nutzer im Test-Panel einen Server als Simulations-Scope, gelten dessen
Tags automatisch mit, sodass tag-gebundene Regeln greifen. Das Panel zeigt
dieselbe Entscheidung, die im echten Betrieb für diesen Server fiele, und nicht
fälschlich „keine Regel matcht". Zusätzliche Tags lassen sich weiterhin manuell
simulieren; doppelte Tags zählen einmal.

## Fund 3 — Größengrenze der Diff-Vorschau

Die Zeilen-Diff-Vorschau (Notiz-Diff, Spec 0019, und Datei-Schreib-Diff,
Spec 0020) ist begrenzt, weil ihre Kosten mit dem Produkt der Zeilenzahlen von
alt und neu wachsen und eine große Datei die Bestätigung sonst einfrieren würde.
Die Vorschau berechnet keinen Zeilen-Diff, wenn

- das Produkt der Zeilenzahlen von altem und neuem Inhalt 2000 × 2000 (vier
  Millionen) übersteigt, oder
- eine der beiden Seiten größer als 256 KB ist (wie beim Lesen von Dateien).

Stattdessen erscheint der Hinweis „Inhalt zu groß für Zeilen-Diff" mit alter und
neuer Größe. Der Schreibvorgang bleibt möglich, die Bestätigung erfolgt dann ohne
detaillierten Diff (analog zu Binärdateien, Spec 0020, Abschnitt 4.2). Die
Grenze gilt nicht nur an der Aufrufstelle: Die Diff-Berechnung selbst verweigert
Eingaben über der Zeilen-Grenze, sodass kein Aufrufer sie umgehen kann. Unter
der Grenze erscheint der normale Diff.

## Fund 4 — Wartende Bestätigungen laufen ab

„Tab schließen = wartende Aktion ablehnen" (Spec 0017, Abschnitt 5) hängt am
Zustand der Oberfläche, der bei einem Neuladen verloren geht. Damit eine
Bestätigung nie ewig wartet, läuft jede wartende Bestätigung im Kern nach **einer
Stunde** ab. Der Ablauf gilt als **Ablehnung**, nie als Genehmigung. Die Aktion
wird nicht ausgeführt, die Sitzung läuft weiter. Im Verlauf steht ein eigener Text
(„Zeitüberschreitung — nicht innerhalb der zulässigen Zeit beantwortet,
automatisch abgelehnt."), kein „Vom Nutzer abgelehnt", und das Protokoll
unterscheidet Ablauf von einer echten Ablehnung. Das gilt für vorgeschlagene
Aktionen und für vorgeschlagene Notiz-Änderungen.

## Fund 5 — Lokaler Pseudo-Server ohne Port-Anzeige

Der lokale Pseudo-Server (Spec 0032) läuft über direkte Prozessausführung und hat
keinen Port. Die Server-Übersicht zeigt für ihn nur `Benutzer@Host`, nie einen
Port; normale Server zeigen `Benutzer@Host:Port` unverändert.

## Sicherheitszusagen

- Fund 1 ändert die Lösch-Reihenfolge nicht (erst Schlüsselbund, dann Datenbank);
  vor dem Löschen steht nur eine Bestätigung.
- Fund 4: Ein Ablauf ist fail-safe eine Ablehnung.
- Keiner dieser Punkte lockert eine Entscheidung der Filter-Engine oder eine
  Bestätigung.
