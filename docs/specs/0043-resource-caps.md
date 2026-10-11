# Spec 0043 — Ressourcengrenzen gegen feindliche oder fehlerhafte Server

Status: umgesetzt
Zweck: Drei Grenzen, die greifen, **während** etwas verarbeitet wird, damit ein feindlicher oder fehlerhafter Server (oder eine bösartige KI-Ausgabe) keine Ressourcen erschöpfen kann: Kommando-Ausgabe, Verschachtelungstiefe beim Analysieren, Länge des Zweitmeinungs-Inhalts.
Bezüge: Spec 0005 (Transport), Spec 0002 (Filter-Engine), Spec 0026 (Risiko-Hinweise, Zweitmeinung), Spec 0039 (Fencing), Spec 0044 (lokaler Pseudo-Server), ADR 0036.

## 1. Gemeinsames Muster

Leitprinzip: **Begrenzen während des Konsumierens, nicht danach.** Bei
Überschreitung wird sauber abgebrochen, mit einer klaren, im UI sichtbaren
Meldung — nie ein Absturz, nie Speichererschöpfung, nie ein Hänger. Die
Richtung ist in allen drei Fällen fail-safe: Keine Grenze macht eine
Entscheidung unsicherer.

## 2. Fund A — Ausgabegrenze beim Kommando

Die Ausgabe eines ausgeführten Kommandos (Standardausgabe und Fehlerausgabe
zusammen) ist auf 2 MB je Ausführung begrenzt. Die Grenze gilt **je
Kommandoausführung**, nicht kumulativ über eine Sitzung.

- Die Bytes werden beim Empfangen gezählt. Sobald die Grenze erreicht ist,
  wird nicht weiter gelesen: Der Rest der Serverausgabe wird verworfen und
  der Kanal geschlossen. Der Speicher wächst nie über die Grenze hinaus,
  auch nicht bei einem Server, der beliebig viel liefert.
- Das Ergebnis ist als **abgeschnitten** markiert. Oberfläche und
  KI-Kontext machen das kenntlich („Ausgabe bei 2 MB abgeschnitten").
- Die Grenze gilt für das Ausführen einzelner Kommandos. Die interaktive
  Terminal-Sitzung streamt fortlaufend ins Terminal und puffert nicht auf
  diese Weise; ihr Rückblick-Puffer ist durch das Terminal selbst begrenzt.
- Für den lokalen Pseudo-Server gilt dieselbe Grenze (Spec 0044).

## 3. Fund B — Tiefengrenze bei verschachtelter Kommando-Substitution

Beim Analysieren verschachtelter `$(...)` und Backticks gilt eine
ausdrückliche Tiefengrenze von 32 Ebenen — großzügig für jeden legitimen
Fall, weit unter jeder Stack-Grenze.

- Wird sie überschritten, wird das Kommando **blockiert** (`Deny`) mit dem
  Grund, dass es zu tief verschachtelt ist und nicht sicher analysiert
  werden konnte. Nie ein unbegrenzter Abstieg, nie automatische Ausführung,
  und auch nie nur eine Bestätigung: Ein gezielt über die Grenze hinaus
  verschachtelter Befehl würde sonst eine Verbieten-Regel für das innere
  Kommando umgehen, weil die Analyse vor dem inneren Kommando abbricht. Ein
  Kommando, das die Analyse bewusst vereitelt, ist verdächtig, nicht
  harmlos (ADR 0036).
- Die Grenze gilt für Filter-Engine (Spec 0002) und Risiko-Klassifizierer
  (Spec 0026) **identisch**; kein Konsument steigt tiefer ab als der
  andere.
- Die Tiefe wird beim Absteigen geprüft, nicht erst nach vollständigem
  Parsen.

## 4. Fund C — Längengrenze für den Zweitmeinungs-Inhalt

Der Inhalt, der an den optionalen Zweitmeinungs-Anbieter geht
(Risiko-Zweitmeinung, Spec 0026; KI-Fencing-Prüfung, Spec 0039), ist auf
16 KB begrenzt.

- Längerer Inhalt wird **vor dem Senden** gekürzt, nie mitten in einem
  Zeichen. Die Kürzung betrifft nur die Zweitmeinung, nicht die
  Ausführung. Die Zweitmeinung erfährt, dass gekürzt wurde.
- Schwärzung (Spec 0016) läuft vor dem Kürzen und dem Senden.
- Da die Zweitmeinung nur eskalieren kann, ist die Kürzung fail-safe: Im
  schlimmsten Fall übersieht die Zweitmeinung etwas im abgeschnittenen
  Teil; das regelbasierte Ergebnis bleibt unberührt.

## 5. Sicherheitszusagen

- Keine der drei Grenzen schwächt eine Sicherheitsentscheidung ab: Fund A
  markiert nur als abgeschnitten, Fund B blockiert, Fund C betrifft nur die
  rein eskalierende Zweitmeinung.
- Ein Kommando knapp **unter** der Tiefengrenze wird normal analysiert (kein
  falscher Alarm), eines darüber wird blockiert, ohne dass die App
  abstürzt.
- Inhalt unter der Zweitmeinungs-Grenze wird unverändert gesendet.

## 6. Nachweis (Testfälle)

- Fund A: Ein Server, der mehr als die Grenze liefert, lässt den Puffer
  nicht über die Grenze wachsen; das Ergebnis ist als abgeschnitten
  markiert.
- Fund B: Verschachtelung über der Grenze ergibt `Deny` mit dem
  beschriebenen Grund, ohne Stack-Überlauf — für Filter-Engine und
  Risiko-Klassifizierer je ein Test; ein Fall mit einer Verbieten-Regel für
  das innere Kommando belegt, dass sie nicht umgangen wird.
- Fund C: Inhalt über der Grenze wird vor dem Zweitmeinungs-Aufruf gekürzt;
  das regelbasierte Ergebnis bleibt unverändert.

## 7. Grenzen

- Die Werte (2 MB, 32 Ebenen, 16 KB) sind feste interne Größen, kein
  Bedienknopf für den Nutzer.
- Ein gesonderter Schutz des Terminal-Rückblick-Puffers über die
  Terminal-eigene Grenze hinaus besteht nicht.
