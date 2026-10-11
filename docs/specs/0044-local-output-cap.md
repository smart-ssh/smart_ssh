# Spec 0044 — Ausgabegrenze für den lokalen Pseudo-Server

Status: umgesetzt
Zweck: Auch Kommandos auf dem lokalen Pseudo-Server haben dieselbe Ausgabegrenze wie Kommandos auf einem entfernten Server.
Bezüge: Spec 0043 (Fund A, Ausgabegrenze), Spec 0032 (lokaler Pseudo-Server), ADR 0130.

## 1. Verhalten

Ein lokales Kommando, das sehr viel ausgibt (etwa das Ausgeben einer
riesigen Datei oder eine Endlosschleife), darf den Speicher der App nicht
erschöpfen. Es gilt dieselbe Grenze wie im entfernten Fall (2 MB je
Ausführung, Spec 0043 Fund A):

- Die Ausgabe wird beim Lesen gezählt. Bei Erreichen der Grenze wird nicht
  weiter gelesen, der Rest verworfen und das lokale Kommando beendet — samt aller
  Pipeline-Glieder und Hintergrundprozesse, die es gestartet hat, nicht nur
  die Shell (ein Prozess, der sich selbst aus der Prozessgruppe löst, wird
  nicht erreicht).
- Das Ergebnis ist als abgeschnitten markiert; Oberfläche und KI-Kontext
  zeigen es genauso an wie im entfernten Fall.
- Zählung und Grenze sind für lokale und entfernte Ausführung dieselben;
  die Standardwerte dürfen nicht auseinanderlaufen.

## 2. Abgrenzung

Die interaktive lokale Terminal-Sitzung ist nicht betroffen. Sie streamt
fortlaufend ins Terminal, das seinen Rückblick selbst begrenzt. Nur das
Ausführen einzelner Kommandos hat die Grenze.

## 3. Sicherheitszusagen

Die Grenze markiert nur als abgeschnitten und schwächt keine
Sicherheitsentscheidung ab (wie Spec 0043 Fund A). Sie hängt an der
Ausführung, nicht an der Server-Identität: In der Sicherheitslogik wird der
lokale Pseudo-Server nicht besonders behandelt.

## 4. Nachweis (Testfälle)

- Ein lokales Kommando, das mehr als die (im Test kleiner gesetzte) Grenze
  ausgibt, hält den Puffer unter der Grenze, und das Ergebnis ist als
  abgeschnitten markiert.
- Ein Test belegt, dass lokale und entfernte Ausführung dieselbe
  Standardgrenze verwenden.
