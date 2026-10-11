# Spec 0045 — Andockpunkte für Erweiterungen im Frontend

Status: umgesetzt
Zweck: Welche Stellen der Oberfläche ein Frontend-Modul (z. B. eine andere
Edition) erweitern kann, und welche Regeln dafür gelten.
Bezüge: Spec 0012 (KI-Dokumente), Spec 0031 (Erststart-Hinweis), Spec 0037
(Editionen), Spec 0038 (Aufbau des Repositorys), Spec 0050
(Einstellungen), ADR 0037, ADR 0123.

## 1. Überblick

Das Frontend hat eine Registry, über die ein Modul beim Start Beiträge
anmeldet. Die Registry ist editionsneutral: Sie kennt keine Editionen,
Freischaltungen, Netzwerk- oder Update-Logik.

## 2. Grundsatz

**Kein Andockpunkt ohne Darstellung.** Jede Beitragsart der Registry hat
eine Stelle in der Oberfläche, die ihre Beiträge tatsächlich anzeigt. Ein
Andockpunkt, der nichts anzeigt, lässt ein Feature unbemerkt unsichtbar
werden und wird deshalb nicht angeboten. Braucht ein Feature eine neue
Beitragsart, kommt sie zusammen mit ihrer Darstellung.

Für jede Beitragsart gilt: Ein Beitrag hat eine Kennung; eine erneute
Anmeldung mit derselben Kennung ersetzt die vorherige.

## 3. Dokument-Aktionen

Ein Modul kann eine Aktion für KI-generierte Dokumente (Spec 0012)
anmelden. Eine Dokument-Aktion hat:

- eine Kennung und eine Beschriftung (z. B. „Als Word speichern"),
- eine Handlung, die beim Klick mit Titel und Markdown-Inhalt des
  Dokuments aufgerufen wird,
- optional die Angabe „gesperrt" mit einer Begründung.

Ob eine Aktion gesperrt ist, entscheidet das anmeldende Modul. Die
Registry und die Dokument-Karte zeigen nur an, was sie bekommen.

## 4. Darstellung der Dokument-Aktionen

- Die Aktionen erscheinen **nur auf der Dokument-Karte** (Spec 0012,
  Abschnitt 3), **neben** „Als Markdown speichern", in der Reihenfolge der
  Anmeldung. Der Markdown-Export bleibt unverändert an seinem Platz.
- Eine aktive Aktion ist klickbar und ruft ihre Handlung auf.
- Eine gesperrte Aktion bleibt sichtbar, ist ausgegraut und nicht
  klickbar; ihre Begründung erscheint als Tooltip.

## 5. Beitragsarten

Die Registry bietet genau drei Beitragsarten an:

- Abschnitte der Einstellungen (Spec 0050),
- Dokument-Aktionen (Abschnitte 3 und 4),
- Erweiterungen des Erststart-Hinweises (Abschnitt 9).

Andockpunkte für eigene Routen, Seitenbereiche oder eine Befehlspalette
gibt es nicht; die App hat dafür keine Darstellung.

## 6. Sicherheitszusagen

- Die Registry bleibt editionsneutral; sie enthält kein Wissen über
  Freischaltungen.
- „Gesperrt" an einer Dokument-Aktion ist nur die Anzeige. Die
  verbindliche Prüfung, ob ein Feature freigeschaltet ist, liegt im
  Befehl, den die Aktion aufruft. Wer die Sperre in der Oberfläche umgeht,
  scheitert dort.

## 7. Akzeptanzfälle

- Anmelden und Auflisten von Dokument-Aktionen; gleiche Kennung ersetzt.
- Die Dokument-Karte zeigt eine angemeldete aktive Aktion (Klick ruft die
  Handlung mit Titel und Inhalt auf) und eine gesperrte (sichtbar, nicht
  klickbar, Begründung als Tooltip).

## 8. (entfallen)

## 9. Erweiterungen des Erststart-Hinweises

Ein Modul kann dem Erststart-Hinweis (Spec 0031, Abschnitt 6) eigene
Elemente hinzufügen.

- Eine Erweiterung hat eine Kennung, eine Rangfolge und eine eigene
  Anzeige. Erweiterungen erscheinen aufsteigend nach Rangfolge, bei
  Gleichstand nach Kennung.
- Eine Erweiterung kann genau eine Handlung anmelden, die einmal
  ausgeführt wird, nachdem der Nutzer den Hinweis bestätigt hat und die
  Bestätigung gespeichert ist. Eine erneute Anmeldung ersetzt die
  vorherige Handlung derselben Erweiterung.
- Was eine Erweiterung anzeigt und tut, verantwortet das anmeldende Modul.
  Optionale Elemente sind beim Anzeigen aus bzw. leer (Spec 0031,
  Abschnitt 6).
- Eine fehlerhafte Erweiterung verhindert den Erststart-Hinweis nicht
  (ADR 0123).
