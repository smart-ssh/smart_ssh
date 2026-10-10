# Spec 0018 — Sudo-Passwort für privilegierte Kommandos

Status: umgesetzt
Zweck: Ein optionales, pro Server hinterlegtes Sudo-Passwort ermöglicht es, dass ein von der KI vorgeschlagenes und freigegebenes `sudo`-/`doas`-Kommando ohne Terminal ausgeführt wird — flüchtig, sichtbar und ohne Secret im Kommandotext.
Bezüge: Spec 0003 (Credentials), Spec 0005 (SSH), Spec 0016 (Redaction/Log), Spec 0020 (Datei schreiben mit Sudo-Fallback), Spec 0068 Teil 3 (Ankündigung des Fallbacks), Spec 0096 und 0101 (Ablage der Secrets).

## 1. Problem

KI-Kommandos laufen über einen nicht-interaktiven Kanal ohne Terminal. Ein
`sudo`, das ein Passwort abfragt, scheitert dort. Das interaktive Terminal hat
dagegen ein echtes Terminal, teilt aber weder Kanal noch Sudo-Zeitstempel mit
den KI-Kommandos.

## 2. Ziel

Ein optionales Sudo-Passwort pro Server, getrennt vom Login-Passwort abgelegt.
Beginnt ein freigegebenes Kommando mit `sudo` oder `doas`, wird das Passwort
**einmalig und flüchtig** über die Standardeingabe dieses einen Aufrufs
übergeben (`sudo -S`) — nie als Umgebungsvariable, nie in einer Datei, nie auf
dem Zielserver abgelegt.

## 3. Abgrenzung

- Erkannt wird nur ein Kommando, das **als Ganzes** mit `sudo` oder `doas`
  beginnt (führender Leerraum ist erlaubt). Ein `sudo` mitten in einer
  Kommandokette (`foo && sudo bar`) wird nicht erkannt und läuft wie ohne
  Passwort, scheitert also, wenn `sudo` ein Passwort verlangt.
- KI-Kanal und interaktives Terminal werden nicht zusammengelegt. Im Terminal
  gibt der Nutzer das Passwort weiterhin selbst ein; es wird dort nie
  automatisch eingespeist.
- Der lokale Pseudo-Server unterstützt die Übergabe über die Standardeingabe nicht.

## 4. Speicherung

- Das Sudo-Passwort hat einen eigenen Platz je Server, getrennt vom
  Login-Passwort und von Passphrasen.
- Server-Formular: Ein neu eingegebener Wert wird gesetzt oder überschreibt den
  alten; ein **leeres** Feld lässt einen vorhandenen Wert **unverändert**.
- Zum Entfernen gibt es eine eigene Aktion; „leer lassen" kann das nicht, weil
  es „unverändert" bedeutet.
- Beim Löschen des Servers wird das Sudo-Passwort mitgelöscht.
- Ob ein Sudo-Passwort hinterlegt ist, kann die Oberfläche erfahren; der Wert
  selbst wird nie an die Oberfläche geliefert.

## 5. Ausführung

Nach Filter-Engine und Bestätigung gilt für das freigegebene Kommando:

1. Beginnt es mit `sudo`/`doas` (Abschnitt 3) **und** ist für die Sitzung ein
   Sudo-Passwort hinterlegt, wird `-S` direkt hinter `sudo`/`doas` eingefügt und
   das Passwort mit abschließendem Zeilenumbruch über die Standardeingabe
   gesendet. Enthält das Kommando bereits ein `-S` oder `-A`, bleibt es
   unverändert und ohne Passwortübergabe.
2. Sonst läuft das Kommando unverändert wie ohne Sudo-Passwort.

- Das Passwort wird beim Verbinden einmal gelesen. Fehlt es, ist das der
  Normalfall und kein Verbindungsfehler. Schlägt das Lesen aus einem anderen
  Grund fehl (z. B. verweigert der Schlüsselbund den Zugriff), läuft die
  Sitzung ohne Sudo-Passwort weiter und der Vorfall wird ohne Fehlertext
  protokolliert.
- Das angezeigte und protokollierte Kommando enthält `-S`, nie das Passwort,
  weil dieses nie Teil des Kommandotexts ist.
- Für das Schreiben von Dateien mit Sudo-Fallback (Spec 0020) gilt dasselbe
  Passwort und dieselbe Ankündigung (Abschnitt 7, Spec 0068 Teil 3).

## 6. Sitzungszustand

- Das Passwort gehört zur Sitzung und wird in ihr nur als geschützter Wert
  gehalten, der nicht in Logs oder Debug-Ausgaben erscheint.
- Es wird nicht mit anderen Sitzungen geteilt.

## 7. Transparenz im Bestätigungsdialog

- Der Bestätigungsdialog zeigt weiterhin genau das vorgeschlagene Kommando
  (ohne `-S`) und ergänzt einen deutlichen Hinweis, wenn Abschnitt 5, Punkt 1
  zutrifft („wird mit hinterlegtem Sudo-Passwort ausgeführt").
- Ob das hinterlegte Passwort verwendet wird, entscheidet das Backend und teilt
  es der Oberfläche mit; die Oberfläche rät nicht.
- Ein Kommando, das das hinterlegte Passwort verwendet, wird **nie automatisch**
  ausgeführt: Eine automatische Ausführung (`AutoExec`) wird auf Bestätigung
  eskaliert, auch wenn eine Erlauben-Regel greift.

## 8. Sicherheitszusagen

- Das Passwort verlässt den Rechner nur über die Standardeingabe genau eines
  `sudo -S`-Aufrufs auf dem bereits authentifizierten Kanal.
- **Redaction:** Das hinterlegte Sudo-Passwort ist als zusätzliches Muster
  Teil der Ausgabe-Redaction der Sitzung. Hintergrund: `sudo -S` liest die
  Zeile nur, wenn es tatsächlich nach einem Passwort fragt. Bei `NOPASSWD`
  oder gültigem Sudo-Zeitstempel erhält das **ausgeführte Programm** die
  Zeile (etwa `sudo cat` oder `sudo tee`) und könnte sie ausgeben. Das
  Passwort erreicht deshalb weder KI-Kontext noch Log im Klartext.
- Das Passwort liegt wie alle Secrets nur in der verschlüsselten Datenbank
  (Spec 0096, Spec 0101).
- Das Passwort ist kein Teil von Kommandotext, Ledger, Chatverlauf oder
  Log-Eintrag.

## 9. Grenzen

- Ein `sudo` in der Mitte einer Kommandokette wird nicht unterstützt
  (Abschnitt 3).
- Das Sudo-Passwort hat kein Ablaufdatum und keine erneute Abfrage; es folgt
  damit dem Modell des Login-Passworts.
