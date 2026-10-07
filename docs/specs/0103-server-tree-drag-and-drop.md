# Spec 0103 — Server und Gruppen per Drag-and-drop verschieben

Status: umgesetzt · Issue: #48
Zweck: Server und Gruppen lassen sich in beiden Server-Bäumen per
Drag-and-drop in eine andere Gruppe oder auf die oberste Ebene verschieben,
statt das Formular zu öffnen und die Gruppen-Auswahl zu ändern.
Review-Priorität: NORMAL (Filter-Engine, Risiko-Klassifizierer, Redaction,
Credentials und KI-Ausführungspfad bleiben unberührt)

## 1. Ist-Stand (vor dieser Spec)

- Die Gruppe eines Servers ändert nur das Gruppen-`<select>` in
  `ServerForm.tsx`, gespeichert über `update_server(id, ServerInput)` — den
  vollständigen Bearbeiten-Weg samt Schlüsselbund-Logik (Spec 0082).
- Die übergeordnete Gruppe einer Gruppe ändert nur `GroupForm` über
  `update_group(id, name, parent_id)` mit der Zyklusprüfung
  `validate_no_cycle` (`crates/app-logic/src/groups.rs`).
- Spec 0008, Abschnitt 2 hatte Drag-and-drop ausdrücklich ausgeschlossen.
  Diese Spec hebt den Ausschluss auf.
- Datenmodell: `servers.group_id`, `groups.parent_id`; Listen sind nach
  Namen sortiert, eine Reihenfolge-Spalte gibt es nicht.

## 2. Verhalten

In **beiden** Bäumen — der Verbinden-Liste (`ServerList.tsx`) und der
Verwalten-Sidebar (`Sidebar.tsx`):

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
  ein gestrichelter Hinweis „Hier ablegen für die oberste Ebene".
- **Hervorhebung.** Ein Ziel, das etwas ändern würde, wird blau umrandet;
  eine Gruppe, die ein Zyklus wäre, rot. Eine Beschriftung folgt dem Zeiger
  und sagt, was ein Loslassen bewirkt. Ziele, die nichts ändern würden,
  werden nicht hervorgehoben.
- **Zyklus.** Die Ablage einer Gruppe auf ihre eigene Untergruppe geht an
  das Backend, das sie mit derselben Prüfung wie das Formular
  (`validate_no_cycle`, Code `GROUP_CYCLE_DETECTED`) ablehnt. Die
  übersetzte Meldung erscheint im Fehlerbereich der jeweiligen Ansicht.
- **Klick bleibt Klick.** Erst ab 5 px Bewegung wird aus einem Klick ein
  Ziehen. Nach einem Ziehen wird der Klick, den der Browser noch liefert,
  verschluckt — ein Ziehen verbindet nicht, wählt nicht aus und klappt
  nicht auf/zu. Escape bricht das Ziehen ohne Ablage ab.
- **Lokaler Pseudo-Server** (Spec 0032): nicht ziehbar und kein Ablageziel.
  Das Backend lehnt ihn zusätzlich ab.
- **Tastatur.** Die Gruppen-Auswahl in `ServerForm`/`GroupForm` bleibt der
  tastaturbedienbare Weg zum Verschieben.
- **Geöffnetes Formular** (Issue #63). Wird in der Verwalten-Ansicht das
  gerade geöffnete Element erfolgreich verschoben, übernimmt das Formular
  nur den neuen Ort: Das Gruppen-Feld (Server) bzw. das Feld für die
  übergeordnete Gruppe (Gruppe) zeigt sofort die neue Gruppe oder „keine"
  (oberste Ebene). Ein späteres Speichern behält damit den neuen Ort. Alle
  anderen ungespeicherten Eingaben bleiben stehen, das Formular lädt nicht
  neu. Ein abgelehntes Verschieben (z. B. Zyklus) ändert im Formular nichts,
  auch nicht das Gruppen-Feld. Das Verschieben eines anderen Elements und
  das anschließende Neuladen der Listen lassen das geöffnete Formular
  unverändert.
- **Reihenfolge** bleibt alphabetisch, keine Migration.

## 3. Befehle

```
move_server_to_group(id: ServerId, group_id: Option<GroupId>)
move_group(id: GroupId, parent_id: Option<GroupId>)
```

Logik in `app_logic::servers::move_server_to_group` und
`app_logic::groups::move_group`; die Tauri-Befehle reichen nur durch.

- `move_server_to_group` ändert ausschließlich `group_id` (und
  `updated_at`). Sie bekommt **keinen** `CredentialStore` und liest oder
  schreibt den Schlüsselbund nicht; die Anmeldeart samt ihrer Verweise
  wird unverändert aus der gespeicherten Zeile übernommen. Eine
  unbekannte Zielgruppe schlägt sichtbar fehl, der lokale Pseudo-Server
  wird abgelehnt.
- `move_group` ändert ausschließlich `parent_id` (und `updated_at`), prüft
  vorher `validate_no_cycle` (`GROUP_SELF_PARENT`/`GROUP_CYCLE_DETECTED`)
  und schreibt bei einer Ablehnung nichts.
- Ist das Element schon am Ziel, schreiben beide nichts.

## 4. Plattformen: Pointer Events statt HTML5-Drag-and-drop

Die App nutzt Tauris native Dateiablage (`onDragDropEvent` im
Dateibrowser). Sie kann HTML5-Drag-Events im Webview abfangen; unter
Windows kommen sie dann nicht an. Das Ziehen ist deshalb mit Pointer Events
umgesetzt (`useTreeDrag.ts`, gleiches Muster wie `useDragResize.ts`):
`pointerdown` merkt sich den Kandidaten, ab der Schwelle übernimmt
`setPointerCapture`, das Ziel unter dem Zeiger bestimmt
`document.elementFromPoint` über das Attribut `data-drop-target`
(`root`, `group:<id>` oder `none`). Pointer Events sind von der
Dateiablage unabhängig und verhalten sich auf macOS, Linux und Windows
gleich. Keine neue Abhängigkeit.

## 5. Sicherheit

Filterregeln gelten global, je Server oder je Tag, nicht je Gruppe; das
Verschieben ändert die Filterung nicht. Es ändert die *effektiven Notizen*
eines Servers (geerbte Gruppennotizen, ADR 0103) — deren Redaction und
Einzäunung bleiben unverändert, sie greifen auf den jeweils aktuellen Baum.

## 6. Tests

- `crates/app-logic/src/servers.rs`: Verschieben in Untergruppe, Gruppe und
  oberste Ebene; Anmeldeart und alle anderen Felder bleiben gleich, der
  Schlüsselbund unberührt; unbekannte Zielgruppe scheitert ohne Änderung;
  lokaler Pseudo-Server abgelehnt.
- `crates/app-logic/src/groups.rs`: Gruppe hinein und zurück auf die
  oberste Ebene (Name bleibt); eigener Nachfahre → `GROUP_CYCLE_DETECTED`
  ohne Änderung; sich selbst → `GROUP_SELF_PARENT`; Server der Gruppe
  bleiben in ihr.
- `treeDrag.test.ts`: Regeln `valid`/`noop`/`cycle`, Zielattribut,
  Aufruf der schmalen Befehle.
- `Sidebar.test.tsx`, `ServerList.test.tsx`: vollständige Zeigergesten
  inklusive lokalem Server, Klick ohne Bewegung, Escape, Zyklus-Meldung.
