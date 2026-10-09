# Spec 0029 — Positionierung der Risiko-Badges

Status: umgesetzt
Zweck: Die Risiko-Badges sitzen in der Kopfzeile der Aktionskarte und reißen den Lesefluss des Kommandos nicht auseinander.
Bezüge: Spec 0026 (Risiko-Einschätzung), Spec 0007 (Aufbau des Bestätigungsdialogs).

## 1. Ziel

Die beiden Risiko-Badges („Server", „Daten", Spec 0026, Abschnitt 4) stehen
nicht als eigener Block über dem Kommando-Text, sondern in derselben Zeile wie
das Label des Bestätigungs-Kastens.

## 2. Anordnung

In der Zeile mit dem Label („Vorgeschlagenes Kommando") bzw. dem Badge der
Filter-Entscheidung (z. B. „Bestätigung nötig") stehen die Risiko-Badges
rechtsbündig am Zeilenende, auf gleicher Höhe wie die Beschriftung. Der
Kommando-Text steht unverändert direkt darunter.

Reihenfolge von links nach rechts: Label — Lücke — Risiko-Badges (Server,
Daten, soweit vorhanden) — Badge der Filter-Entscheidung, falls in derselben
Zeile dargestellt. Sind beide Achsen ohne Risiko, nimmt der Bereich keinen
Platz ein: kein leerer Zwischenraum, kein Layout-Sprung. Stammt die Aktion
aus einem externen Tool (Spec 0028), steht dessen Herkunfts-Badge ebenfalls in
dieser Zeile, vor den Risiko-Badges. Der Hinweistext (Spec 0026, Abschnitt 4)
steht als Fußnote unter dieser Zeile.

## 3. Umfang

Die Anordnung gilt überall, wo die Badges erscheinen: in der Aktionskarte im
Chat und im Bestätigungsdialog. Die Berechnung der Einschätzung bleibt davon
unberührt (Spec 0026).

## 4. Prüffall

Eine strukturelle Prüfung der Oberfläche (keine reine Bildschirmaufnahme): Die
Badges befinden sich im selben Zeilen-Container wie das Aktions-Label, nicht
mehr als eigener Block oberhalb des Kommandos.
