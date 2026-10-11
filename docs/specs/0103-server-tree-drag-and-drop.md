# Spec 0103 — Server und Gruppen per Drag-and-drop verschieben

Status: umgesetzt
Zweck: Server und Gruppen lassen sich in beiden Server-Bäumen per Drag-and-drop in eine andere Gruppe oder auf die oberste Ebene verschieben, statt das Formular zu öffnen und die Gruppen-Auswahl zu ändern.
Bezüge: Spec 0008 (Server-Verwaltung; der dortige Ausschluss von Drag-and-drop gilt nicht mehr), Spec 0033 (gruppierte Server-Übersicht), Spec 0032 (lokaler Pseudo-Server), Spec 0082 (Bearbeiten und Anmeldeart), ADR 0103 (effektive Notizen).
Review-Priorität: NORMAL (Filter-Engine, Risiko-Klassifizierer, Redaction, Credentials und KI-Ausführungspfad bleiben unberührt)

## 1. Ausgangspunkt

Die Gruppe eines Servers lässt sich im Server-Formular ändern, die
übergeordnete Gruppe einer Gruppe im Gruppen-Formular (mit Zyklusprüfung).
Das Verschieben per Drag-and-drop ist ein zusätzlicher, schmalerer Weg: Er
ändert nur den Ort, nie die übrigen Felder. Listen bleiben nach Namen
sortiert; es gibt keine eigene Reihenfolge.

## 2. Verhalten

In **beiden** Bäumen — der Verbinden-Liste und der Verwalten-Sidebar:

| Gezogen | Abgelegt auf | Ergebnis |
|---|---|---|
| Server | Gruppe (auch Untergruppe, auch zugeklappt) | Server liegt in dieser Gruppe |
| Server | oberste Ebene | Server ohne Gruppe |
| Gruppe | andere Gruppe | wird deren Untergruppe |
| Gruppe | oberste Ebene | wird Gruppe der obersten Ebene |
| Gruppe | eigene Untergruppe (beliebig tief) | abgelehnt, sichtbare Meldung, nichts ändert sich |
| beliebig | wo es schon liegt / auf sich selbst | nichts passiert |
| beliebig | kein Ablageziel | nichts passiert |

- **Ablageziele.** Eine Gruppe ist mit ihrer ganzen Fläche Ziel
  (Kopfzeile, Untergruppen und Server darin; das innerste Ziel gewinnt).
  Oberste Ebene: in der Verbinden-Liste der Abschnitt „Ohne Gruppe", der
  während des Ziehens auch dann erscheint, wenn er sonst leer wäre; in der
  Sidebar die gesamte freie Baumfläche. Beim Ziehen erscheint zusätzlich
  ein gestrichelter Hinweis „Hier ablegen für die oberste Ebene (ohne
  Gruppe)".
- **Hervorhebung.** Ein Ziel, das etwas ändern würde, wird blau umrandet;
  eine Gruppe, die ein Zyklus wäre, rot. Eine Beschriftung folgt dem Zeiger
  und sagt, was ein Loslassen bewirkt („→ in diese Gruppe verschieben",
  „→ oberste Ebene (ohne Gruppe)", bzw. „Eine Gruppe kann nicht in ihre eigene
  Untergruppe verschoben werden"). Ziele, die nichts ändern würden, werden
  nicht hervorgehoben.
- **Zyklus.** Die Ablage einer Gruppe auf ihre eigene
  Untergruppe wird mit derselben Prüfung abgelehnt wie im Gruppen-Formular.
  Die übersetzte Meldung erscheint im Fehlerbereich der jeweiligen Ansicht;
  gespeichert wird nichts.
- **Klick bleibt Klick.** Erst ab 5 px Bewegung wird aus einem Klick ein
  Ziehen. Nach einem Ziehen wird der Klick, den der Browser noch liefert,
  verschluckt — ein Ziehen verbindet nicht, wählt nicht aus und klappt
  nicht auf oder zu. Escape bricht das Ziehen ohne Ablage ab.
- **Lokaler Pseudo-Server** (Spec 0032): nicht ziehbar und kein Ablageziel.
  Auch beim Verschieben wird er abgelehnt, nicht nur in der Oberfläche.
- **Tastatur.** Die Gruppen-Auswahl im Server- und im Gruppen-Formular bleibt
  der tastaturbedienbare Weg zum Verschieben.
- **Geöffnetes Formular.** Wird in der Verwalten-Ansicht das gerade
  geöffnete Element erfolgreich verschoben, übernimmt das Formular nur den
  neuen Ort: Das Gruppen-Feld (Server) bzw. das Feld für die übergeordnete
  Gruppe (Gruppe) zeigt sofort die neue Gruppe oder „keine" (oberste Ebene).
  Ein späteres Speichern behält damit den neuen Ort. Alle anderen
  ungespeicherten Eingaben bleiben stehen, das Formular lädt nicht neu. Ein
  abgelehntes Verschieben (z. B. Zyklus) ändert im Formular nichts, auch nicht
  das Gruppen-Feld. Das Verschieben eines anderen Elements und das
  anschließende Neuladen der Listen lassen das geöffnete Formular
  unverändert.
- **Reihenfolge** bleibt alphabetisch.

## 3. Was ein Verschieben ändert

- Beim Server wird ausschließlich die Gruppe (und der Änderungszeitpunkt)
  geändert. Anmeldeart, Verweise auf gespeicherte Geheimnisse und alle
  anderen Felder bleiben wie gespeichert; der Schlüsselbund bzw. die
  Secret-Ablage wird weder gelesen noch geschrieben. Eine unbekannte
  Zielgruppe schlägt sichtbar fehl.
- Bei der Gruppe wird ausschließlich die übergeordnete Gruppe (und der
  Änderungszeitpunkt) geändert. Vorher läuft die Zyklusprüfung (Gruppe als
  eigene übergeordnete Gruppe und eigene Nachfahren werden abgelehnt); bei
  einer Ablehnung wird nichts geschrieben. Die Server der Gruppe bleiben in
  ihr.
- Liegt das Element schon am Ziel, wird nichts geschrieben.

## 4. Plattformen

Das Ziehen nutzt Zeigerereignisse statt des HTML5-Drag-and-drop. Die App
fängt die native Dateiablage des Fensters ab (Dateibrowser); unter Windows
kämen HTML5-Drag-Ereignisse im Webview dann nicht an. Zeigerereignisse sind
davon unabhängig und verhalten sich auf macOS, Linux und Windows gleich.

## 5. Sicherheitszusagen

Filterregeln gelten global, je Server oder je Tag, nicht je Gruppe; das
Verschieben ändert die Filterung nicht. Es ändert die *effektiven Notizen*
eines Servers (geerbte Gruppennotizen, ADR 0103) — deren Redaction und
Einzäunung bleiben unverändert, sie greifen auf den jeweils aktuellen Baum.
Das Verschieben berührt keine Geheimnisse.

## 6. Grenzen

- Keine frei wählbare Reihenfolge der Einträge.
- Kein Drag-and-drop zwischen Fenstern oder von außerhalb der App.
- Für Tastaturnutzer gibt es kein Ziehen; sie nutzen die Formulare.
