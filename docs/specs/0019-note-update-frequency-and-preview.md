# Spec 0019 — Häufigere Notiz-Vorschläge mit Änderungs-Vorschau

Status: umgesetzt
Zweck: Die KI schlägt Notiz-Aktualisierungen schon während der Sitzung vor,
und jeder Vorschlag zeigt vor der Bestätigung, was sich an der Notiz
ändert.
Bezüge: Spec 0003 (Notizen, Bestätigungspflicht), Spec 0009
(Farbpalette), Spec 0010 (Vorschlag beim Beenden), Spec 0023
(Ziel-Kennzeichnung), Spec 0030 (Diff in der Historie).

## 1. Überblick

Ein Notiz-Vorschlag der KI enthält immer den vollständigen neuen Notiztext
und wird nie automatisch übernommen (Spec 0003). Diese Spec regelt zwei
Dinge: wie oft die KI vorschlagen soll (Abschnitt 2) und wie der Nutzer die
Änderung vor der Bestätigung sieht (Abschnitte 3 und 4).

## 2. Instruktion an die KI

Die KI wird angehalten, während der **gesamten** Sitzung auf dauerhafte
Fakten zu achten, die künftigen Sitzungen Erkundungs-Kommandos ersparen und
Fehler verhindern, und dafür eine Notiz-Aktualisierung vorzuschlagen, bei
Bedarf mehrfach pro Sitzung und nicht erst am Ende, sobald sich ein
dauerhafter Fakt als neu oder geändert zeigt. Für bloße Ereignisse
(„nginx neu gestartet“) schlägt sie nichts vor. Sie soll nichts
wiederholen, was schon in den Notizen steht.

### 2.1 Notiz-Format

Dieselbe Beschreibung gilt für den System-Prompt, die Anregung beim
Verbindungsende, die Beschreibung des Notiz-Werkzeugs und die Anweisung zum
Zusammenfassen einer großen Notiz, in Deutsch und Englisch je nach
Sprache der Oberfläche. Server- und Gruppen-Notizen folgen demselben
Format (Gruppen: Fakten, die für alle Server der Gruppe gelten).

- Feste Abschnitte in dieser Reihenfolge, leere entfallen: Constraints,
  Platform, Services, Paths (nie Werte von Geheimnissen), Runtime &
  Network, Quirks & Workarounds, Conventions & Users, Scripts & Aliases,
  Recent changes.
- Recent changes: höchstens 5 einzeilige Einträge `YYYY-MM-DD <dauerhafte
  Änderung>`, neueste zuerst, der älteste fällt weg; nur Änderungen mit
  dauerhafter Wirkung.
- Ein Fakt pro Zeile, knapp im Stil `key: value`. Ein geänderter Fakt wird
  an Ort und Stelle aktualisiert. Widerspricht eine Beobachtung der Notiz,
  gilt die Beobachtung. Eine vom Nutzer geschriebene Constraint ändert oder
  entfernt die KI nicht selbst, sie fragt.
- Nie gespeichert werden: Geheimnis-Werte, flüchtiger Zustand, rohe
  Kommandoausgaben, Routine-Aktionen, Fakten, die ein naheliegendes Kommando
  verrät und die die vorgeschlagenen Kommandos nicht ändern (außer
  Plattform-Fakten), Spekulation.
- Die ganze Notiz bleibt unter 7.500 Zeichen; darüber entfällt zuerst
  Recent changes, dann die am wenigsten nützlichen Quirks, nie Constraints.
- Verlässt sich eine destruktive Aktion auf einen Notiz-Fakt, den die
  Sitzung noch nicht bestätigt hat, prüft die KI ihn vorher mit einem
  günstigen, lesenden Kommando.
- Eine bestehende Notiz in einem anderen Format wird nicht migriert; der
  nächste Vorschlag der KI schreibt sie in das neue Format um (als Diff,
  vom Nutzer zu bestätigen).

Das Format ist reine Führung der KI. Die App prüft, verwirft oder kürzt
Vorschläge deswegen nicht; Schwärzung, Bestätigung und der Hinweis auf
große Notizen (Spec 0057) bleiben unverändert.

Es gibt keine feste Mindestgröße für einen Vorschlag. Ob eine Änderung
sinnvoll ist, entscheidet die KI nach dieser Instruktion; jeder Vorschlag
bleibt bestätigungspflichtig.

## 3. Grundlage der Vorschau

- Die KI liefert nur den neuen Notiztext, keinen Diff und keine
  Zusammenfassung der Änderung. Die Vorschau entsteht aus dem **aktuell
  gespeicherten** Inhalt des Ziels, nicht aus einer Selbstauskunft der KI.
- Die App liest den gespeicherten Inhalt des aufgelösten Ziels (Server
  oder Gruppe) in dem Moment, in dem sie den Vorschlag anzeigt. Das gilt
  für Vorschläge im Chat und für den Vorschlag beim Beenden (Spec 0010)
  gleichermaßen.
- Lässt sich das Ziel nicht auflösen (z. B. Server inzwischen gelöscht)
  oder ist die bisherige Notiz leer, zeigt die Vorschau den neuen Inhalt
  ohne Hervorhebung. Ein Fehler entsteht dadurch nicht.

## 4. Darstellung der Vorschau

- Zeilenbasierter Vergleich zwischen bisherigem und neuem Inhalt.
- **Kurz:** Nur geänderte Zeilen werden gezeigt, unveränderte entfallen.
  Hinzugefügte Zeilen sind grün, entfernte rot und durchgestrichen, in der
  Palette aus Spec 0009.
- Dieselbe Darstellung gilt an beiden Stellen: in der Aktionskarte eines
  Notiz-Vorschlags im Chat und in der Benachrichtigung beim Beenden
  (dort im aufgeklappten Zustand nach „Anzeigen").

## 5. Grenzen

- Kein wort- oder zeichengenauer Vergleich, nur zeilenweise.
- Die Vorschau ist reine Anzeige. Gespeichert wird bei Annahme der
  vollständige neue Text als neue Revision; die Notiz-Historie bleibt davon
  unberührt.
