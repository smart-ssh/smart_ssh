# Spec 0012 — KI-generierte Dokumente

Status: umgesetzt
Zweck: Die KI kann im Chat ein formatiertes Dokument liefern (z. B. eine
Analyse), das der Nutzer als Markdown-Datei speichern kann.
Bezüge: Spec 0003 (KI-Aktionen), Spec 0006 (Aktionen der KI), Spec 0037
(Editionen), Spec 0045 (weitere Dokument-Aktionen), Spec 0057
(Sitzungsverlauf), ADR 0031, ADR 0037.

## 1. Überblick

Bittet der Nutzer die KI um ein Dokument („gib mir ein Dokument mit der
Analyse"), liefert die KI einen Titel und strukturierten Markdown-Inhalt.
Der Inhalt erscheint als eigene Karte im Chat.

## 2. Die Aktion „Dokument erzeugen"

- Ein Dokument betrifft weder SSH noch den Server. Es durchläuft **nicht**
  die Filter-Engine (Spec 0002) und braucht keine Bestätigung, weil dabei
  nichts Dauerhaftes passiert.
- Ein Dokument wird **nie automatisch** auf die Festplatte geschrieben.
  Eine Datei entsteht erst durch eine ausdrückliche Aktion des Nutzers
  (Abschnitt 3).
- Die Aktion steht der KI im Chat immer zur Verfügung; die KI entscheidet
  selbst, wann ein Dokument passt. In Neben-Anfragen (z. B. Spec 0010) ist
  sie nicht verfügbar.
- Ein Dokument zählt nicht als ausgeführte Aktion: Es löst keine
  automatische Folgerunde der KI aus.

## 3. Ablauf und Darstellung

1. Die KI liefert ein Dokument. Es erscheint sofort als eigene Karte im
   Chatverlauf: Titel, Kennzeichnung „Dokument generiert" und der Inhalt als
   gerendertes Markdown (nicht als Rohtext).
2. Die Karte hat den Knopf **„Als Markdown speichern"**. Daneben stehen
   Aktionen, die eine Erweiterung registriert hat (Spec 0045).
3. „Als Markdown speichern" öffnet den nativen Speichern-Dialog,
   vorbelegt mit einem aus dem Titel abgeleiteten Dateinamen mit der Endung
   `.md`. Zeichen, die nicht auf allen unterstützten Betriebssystemen in
   Dateinamen funktionieren, werden dabei durch Leerzeichen ersetzt und
   mehrfache Leerzeichen zusammengefasst. Bleibt nichts übrig, heißt die
   Datei `Dokument.md`.
4. Geschrieben wird erst, wenn der Nutzer den Dialog bestätigt. Bricht er
   ab, passiert nichts. Nach dem Speichern zeigt die Karte „Als Markdown
   exportiert".

## 4. Weitere Exportformate

Die Community-Edition speichert Dokumente nur als Markdown. Weitere Formate
sind nicht Teil dieser Edition (Spec 0037); sie können über den Andockpunkt
für Dokument-Aktionen hinzukommen (Spec 0045).

## 5. Dokumente im Gesprächsverlauf

Der Dokumentinhalt wird wie eine normale Textantwort der KI in den
Gesprächsverlauf und in das Sitzungsprotokoll übernommen. Die KI kann sich
in der weiteren Unterhaltung darauf beziehen („ergänze im Dokument noch
Abschnitt X").

## 6. Grenzen

- Kein PDF- oder Word-Export in dieser Edition.
- Ein Dokument wird nicht im Chat bearbeitet; Änderungen entstehen durch
  ein neues Dokument der KI.
