# Spec 0023 — Ziel-Kennzeichnung bei Notiz-Vorschlägen

Status: umgesetzt
Zweck: Jeder Notiz-Vorschlag zeigt deutlich, für welchen Server oder welche
Gruppe er gilt, unabhängig davon, was gerade auf dem Bildschirm ist.
Bezüge: Spec 0003 (Notiz-Ziele), Spec 0010 (Vorschlag beim Beenden), Spec
0016 (Zielauflösung), Spec 0017 (Sitzungs-Tabs), Spec 0019
(Änderungs-Vorschau).

## 1. Hintergrund

Ein Vorschlag für Server A konnte erscheinen, während der Nutzer Server B
offen hatte, ohne dass erkennbar war, worauf er sich bezieht. Gespeichert
wurde er korrekt bei A; es fehlte nur die Kennzeichnung. Grundsatz der App:
Der Nutzer muss bei jeder Bestätigung eindeutig erkennen, worauf sie sich
bezieht.

## 2. Ziel

Jede Darstellung eines Notiz-Vorschlags, als Aktionskarte im Chat wie als
Benachrichtigung beim Beenden, zeigt **immer und deutlich** den Namen des
Ziels, nicht nur den Notizinhalt.

## 3. Kennzeichnung

- Die App liefert mit jedem Notiz-Vorschlag den Namen des aufgelösten
  Ziels (Server- oder Gruppenname) mit, aus derselben Zielauflösung, die
  auch beim Speichern gilt.
- **Benachrichtigung beim Beenden:** Der Name steht im Titel, nicht nur im
  Fließtext: „Notiz-Vorschlag für Server „…"" bzw.
  „Gruppen-Notiz-Vorschlag für „…"".
- **Aktionskarte im Chat:** Der Name steht in der Beschriftung, **auch
  wenn** es der Server der eigenen Sitzung ist: „Notiz aktualisieren:
  Server „…"" bzw. „Gruppen-Notiz aktualisieren: „…"". Konsistenz geht hier
  vor dem Vermeiden von Wiederholung.
- Ein Vorschlag für eine **Gruppe** ist immer ausdrücklich als
  Gruppen-Notiz beschriftet, damit Server und Gruppe nicht verwechselt
  werden.
- Lässt sich der Name nicht ermitteln (z. B. Ziel inzwischen gelöscht),
  steht an seiner Stelle „unbekanntes Ziel", nie ein leerer Platz.

## 4. Akzeptanzfall

Ein Notiz-Vorschlag für Server A kommt an, während im Frontend Server B als
aktuell gilt: Die Anzeige nennt nachweislich Server A, nicht B und nicht
gar keinen Namen.
