# Spec: Positionierung der Risiko-Indikatoren

Status: Entwurf
Modul: `frontend/` (Bestätigungsdialog/Aktionskarte)
Abhängigkeiten: Risiko-Indikatoren (Spec 0026), Bestätigungsdialog-Aufbau
(Spec 0007, Abschnitt 7)

## 1. Problem

Die beiden Risiko-Badges ("Server"/"Daten", Spec 0026, Abschnitt 4) sitzen
aktuell oberhalb des Kommando-Textblocks. Das reißt den Lesefluss auseinander
und nimmt Platz an einer Stelle weg, die eigentlich für das Kommando selbst
reserviert sein sollte.

## 2. Ziel

Die Risiko-Badges wandern in dieselbe Zeile wie das Label des
Bestätigungs-Kastens (in der bestehenden Struktur die Zeile mit
"Vorgeschlagenes Kommando" bzw. der Filter-Engine-Entscheidungs-Badge, z. B.
"muss bestätigt werden" — siehe Spec 0007, Abschnitt 7 / die
`demo-action-row`-Struktur), rechtsbündig ans Ende dieser Zeile, auf
gleicher Höhe wie die bestehende Beschriftung. Der Kommando-Textblock selbst
bleibt unverändert direkt darunter, ohne die Badges davor.

Reihenfolge in der Zeile (von links nach rechts): Label ("Vorgeschlagenes
Kommando") — [Lücke] — Risiko-Badges (Server, Daten, falls vorhanden) —
Filter-Engine-Entscheidungs-Badge (Allow/Confirm/Deny-Farbe, falls in
derselben Zeile dargestellt). Sind keine Risiken erkannt (beide Achsen
`None`), nimmt der Bereich keinen Platz ein — kein leerer Zwischenraum,
kein Layout-Sprung.

## 3. Umfang

Reine UI-Positionsänderung, keine Änderung an der Berechnung/Logik der
Risiko-Einschätzung selbst (Spec 0026 bleibt fachlich unverändert). Gilt für
alle Stellen, an denen die Badges aktuell erscheinen: reguläre
Chat-Aktionskarte und Bestätigungsdialog gleichermaßen.

## 4. Test

Visueller/struktureller Test (Snapshot oder einfache DOM-Prüfung): Badges
befinden sich im selben Zeilen-Container wie das Aktions-Label, nicht mehr
als eigener Block oberhalb des Kommandos.
