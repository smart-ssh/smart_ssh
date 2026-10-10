# Spec 0027 — Abbruch lang laufender KI-Kommandos

Status: umgesetzt
Zweck: Ein von der KI vorgeschlagenes Kommando, das nicht von selbst endet (`tail -f`, `journalctl -f`, `watch`), blockiert nicht für immer die Sitzung: der Nutzer kann die Verbindung zu genau diesem Kommando trennen und bekommt die bis dahin gesammelte Ausgabe als Ergebnis.
Bezüge: Spec 0005 (Einzelkommando), Spec 0006 (Redaction), Spec 0007 (Kernschleife, Bestätigung), Spec 0018 (Sudo-Passwort), Spec 0020 (Dateikanal), Spec 0021 (automatische Folgerunden), Spec 0024 (Texte), Spec 0032 (Localhost), Spec 0039 (Fencing), Spec 0043 (Ausgabegrenze), Spec 0066 (Stopp), ADR 0057.

## 1. Ziel

Ein Kommando wartet bis zum Ende seines Kanals. Bei einem Kommando, das nie
endet, wäre das für immer; solange es läuft, halten weitere Kommandos und das
Öffnen eines Terminals derselben Sitzung an.

Diese Spec ergänzt eine **manuelle** Abbruchmöglichkeit: Läuft ein
KI-vorgeschlagenes Kommando länger als 5 Sekunden, erscheint an der
Aktionskarte ein Indikator mit einer Schaltfläche. Ein Klick schließt
**ausschließlich den Kanal dieses einen Kommandos** — nicht die SSH-Verbindung,
nicht die Sitzung, nicht Terminal oder Dateizugriff — und liefert die bis dahin
gesammelte Ausgabe auf demselben Weg zurück in den Chat wie ein regulär
beendetes Kommando.

**Nicht Teil dieser Spec:**

- **Kein automatischer Timeout.** Ein legitim lang laufendes Kommando
  (z. B. `apt upgrade`) würde von einer geratenen Zeitgrenze fälschlich
  beendet. Der Abbruch ist immer eine bewusste Nutzeraktion.
- **Kein Live-Mitlesen der KI.** Die KI sieht immer nur das Ergebnis eines
  abgeschlossenen Aufrufs. Was gebraucht wird, ist ein *begrenzter*
  Ausschnitt; den liefert der Abbruch.
- **Kein garantiertes Beenden des Prozesses** auf dem Server (Abschnitt 3.4).

## 2. Lauf-Indikator

**2.1 Wann.** Sobald ein vorgeschlagenes Kommando tatsächlich ausgeführt wird —
sofort bei automatischer Ausführung, sonst nach „Ausführen" bzw. dem
Ausführen eines bearbeiteten Vorschlags oder dem Akzeptieren mit Regel —
läuft in der Oberfläche ein Zähler. Liegt nach 5 Sekunden noch kein Ergebnis
vor, erscheint an der Aktionskarte:

- ein pulsierender Punkt und der Text „läuft seit {n}s…" (Sekunden, sekündlich
  aktualisiert),
- die Schaltfläche **„Verbindung zu diesem Kommando trennen"**.

Die Beschriftung sagt bewusst nicht „Kommando stoppen", weil sie keinen
garantierten Kill verspricht (Abschnitt 3.4).

**2.2 Nur Kommandos.** Der Indikator erscheint nur für vorgeschlagene
Shell-Kommandos. Lesen und Schreiben von Dateien läuft über den
Dateikanal (Spec 0020) und ist nicht Gegenstand dieser Spec; Notizvorschläge
und Dokumente haben kein Remote-Kommando dahinter.

**2.3 Nach dem Klick.** Die Schaltfläche wird deaktiviert und zeigt „Wird
getrennt…", bis das reguläre Ergebnis eintrifft. Schlägt die Anfrage selbst
fehl, wird sie wieder bedienbar.

**2.4 Folgerunden.** Der Indikator erscheint unabhängig davon, ob die Aktion
durch eine Nutzernachricht oder eine automatische Folgerunde (Spec 0021)
ausgelöst wurde.

## 3. Abbruch

**3.1 Wirkung.** Der Abbruch betrifft nur den Kanal dieses Kommandos. Die
Verbindung, die Sitzung und andere Kanäle bleiben unberührt. Er wirkt auch bei
Kommandos, die mit dem hinterlegten Sudo-Passwort laufen (Spec 0018).

**3.2 Ergebnis.** Das Ergebnis trägt die bis zum Abbruch eingetroffene
Standard- und Fehlerausgabe. Der Exit-Code bleibt leer, das Ergebnis ist als
„abgebrochen" markiert. Das Ergebnis läuft durch dieselben Schritte wie jedes
andere: Redaction vor Anzeige, Verlauf und KI-Anfrage, Eintrag im
Sitzungsverlauf mit dem Vermerk des Abbruchs, Fortsetzung des Chats.

**3.3 Kontext für die KI.** Bei einem abgebrochenen Kommando enthält der
KI-Kontext einen ausdrücklichen Hinweis, dass der Nutzer das Kommando manuell
abgebrochen hat, die Ausgabe unvollständig ist und der fehlende Exit-Code kein
Fehler ist — damit die KI ihn nicht als Kommandofehler liest und denselben
Befehl erneut vorschlägt. Ist die Ausgabe zusätzlich durch die Ausgabegrenze
(Spec 0043) abgeschnitten, steht auch das im Kontext.

**3.4 Reichweite.** Beim Abbruch sendet Smart SSH zunächst ein
SSH-Unterbrechungssignal an den Kanal — vom Server optional unterstützt — und
schließt den Kanal danach in jedem Fall. Das beendet zuverlässig **nur das
lokale Warten**. Dass der Prozess auf dem Server endet, ist nicht garantiert:
anders als im Terminal gibt es im Einzelkommando kein steuerndes Terminal,
`Ctrl+C` erreicht den Prozess nicht von selbst. Die meisten Werkzeuge beenden
sich zeitnah, sobald ihr nächster Schreibversuch auf die geschlossene Leitung
scheitert; ein Versprechen ist das nicht. Die Oberfläche formuliert deshalb
zurückhaltend.

**3.5 Zeitliche Überschneidung.** Kommt der Klick, nachdem das Kommando schon
von selbst geendet hat, wird er ohne Fehlermeldung ignoriert; das reguläre
Ergebnis gilt.

**3.6 Stopp vor dem Start.** Ein Stopp (Spec 0066), der eine noch nicht
gestartete automatische Ausführung verhindert, meldet sie über denselben
Weg als abgebrochen mit leerer Ausgabe; die Karte zeigt dann „abgebrochen"
statt dauerhaft „läuft".

## 4. Darstellung

- Der Indikator sitzt an der Aktionskarte.
- Im Ergebnis steht bei Abbruch statt der Exit-Code-Zeile: „⚠ Manuell
  abgebrochen — Ausgabe möglicherweise unvollständig, kein regulärer
  Exit-Code." Die bis dahin gesammelte Ausgabe steht darunter.
- Alle Texte laufen über das Übersetzungssystem (Spec 0024).

## 5. Abgrenzung

- Gilt nur für vorgeschlagene Kommandos. Datei-Aktionen (Spec 0020) haben
  kein bekanntes „hängt für immer"-Muster und sind ausgenommen.
- Kein automatischer Timeout und kein erzwungenes Beenden des Prozesses auf
  Prozessebene.
- Die 5-Sekunden-Schwelle ist fest.

## 6. Sicherheitszusagen

- Der Abbruch kann nichts ausführen und nichts freigeben; er beendet nur das
  Warten auf ein bereits bestätigtes Kommando.
- Die Ausgabe eines abgebrochenen Kommandos wird genauso redigiert und
  gefenct wie die eines regulär beendeten (Spec 0006, Spec 0039).
- Ein Abbruch ist im Verlauf sichtbar; die KI kann ihn nicht mit einem
  Kommandofehler verwechseln.

## 7. Grenzen

- **Localhost ist nicht abbrechbar.** Für den lokalen Pseudo-Server
  (Spec 0032) erscheint der Indikator zwar, der Abbruch greift aber nicht: das
  Kommando läuft bis zu seinem Ende oder der Ausgabegrenze weiter. Ein nie
  endendes Kommando auf Localhost lässt sich damit nicht beenden.
- **Kein Beenden des Prozesses garantiert.** Siehe Abschnitt 3.4.
- **Die Schwelle ist nicht einstellbar.**
