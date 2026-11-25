# Spec 0117 — Filterregeln als teilbare Regelpakete importieren und exportieren

Status: angenommen · Issue: #1577
Zweck: Ein Nutzer exportiert ausgewählte Filterregeln in eine portable Regelpaket-Datei und importiert ein Paket von jemand anderem (z. B. ein Community-Paket für Debian, Proxmox, Kubernetes-Knoten oder Docker-Hosts), nachdem eine Vorschau gezeigt hat, was das Paket erlauben würde. Import und Export gehören zum freien Kern. Regelpakete dienen dem Teilen von Regeln, nicht der Sicherung.
Review-Priorität: ERHÖHT (importierte Allow-Regeln erweitern, was ohne Rückfrage läuft; Filterregeln, Erweiterungsfassade)

Bezüge: Spec 0002 (Filter-Engine, Vorrang, Hard-Blacklist), Spec 0009 (Regelverwaltung, §7 verweist hierher), Spec 0011 (Schnellregel, Batch-Speichern), Spec 0037 §5 (Regel-Herkunft `Builtin` / `Organization` / `User`), Spec 0038 (Verdrahtung, Erweiterungsfassade), Spec 0075 (Import mit Vorschau als Vorbild), Spec 0077 (ungültige Muster werden beim Speichern abgewiesen), Spec 0109 (verschlüsseltes Voll-Backup), `THREAT-MODEL.md` Abschnitt 6 („Allow-Regeln sind deine Entscheidung").

## 1. Ausgangslage und Abgrenzung

- Eine Regel besteht aus Mustertyp (Glob, Regex, exakt), Muster, Aktion (Allow, Confirm, Deny), Geltungsbereich (global, Server, Schlagwort) und Priorität. Spec 0009 §7 schloss Import und Export von Regelsätzen bisher aus; diese Spec ersetzt diese Grenze.
- **Regelpakete sind zum Teilen**, nicht zur Sicherung. Wer alle Daten (auch Server, Zugangsdaten, Regeln mit Server-Bezug) auf eine andere Maschine bringen will, nutzt das verschlüsselte Voll-Backup (Spec 0109).
- **Sicherheitsgrundsatz:** Eine importierte Allow-Regel erweitert, was ohne Rückfrage läuft. Deshalb ist der Import ein bewusster, zweistufiger Vorgang (Vorschau, dann Anwenden), und Allow-Regeln müssen einzeln angehakt werden (§5).
- Die Auswertung der Filter-Engine (`FilterEngine::evaluate`), ihre Vorrangregeln und die Hard-Blacklist bleiben **unverändert**. Importierte Regeln sind gewöhnliche lokale Regeln.

## 2. Dateiformat

### 2.1 Aufbau

Ein Paket ist **eine UTF-8-JSON-Datei**. Dateiendung `.json`; der Speichern-Dialog schlägt `<paket-id>.rules.json` vor. Die Datei trägt ein Formatkennzeichen, damit sie sich von anderem JSON unterscheiden lässt. Beispiel:

```json
{
  "format": "smart-ssh-rule-pack",
  "format_version": 1,
  "pack": {
    "id": "community.debian-basics",
    "name": "Debian-Grundregeln",
    "description": "Lesende Befehle erlauben, Paketentfernung bestätigen.",
    "version": "1.2.0",
    "homepage": "https://example.org/rule-packs"
  },
  "rules": [
    { "pattern_type": "glob",  "pattern": "systemctl status *", "action": "allow",
      "scope": { "type": "global" }, "priority": 0 },
    { "pattern_type": "regex", "pattern": "^apt(-get)? (remove|purge) ", "action": "confirm",
      "scope": { "type": "tag", "tag": "production" }, "priority": 10 }
  ]
}
```

### 2.2 Felder

- `format`: genau `smart-ssh-rule-pack`. Anderer oder fehlender Wert → die Datei ist kein Regelpaket (klare Meldung).
- `format_version`: ganze Zahl, hier `1`. **Eine unbekannte (auch höhere) Version wird abgelehnt** mit einer Meldung, die die gefundene und die unterstützte Version nennt; es wird nichts gelesen oder gespeichert.
- `pack.id`: Pflicht, nicht leer, höchstens 100 Zeichen aus `a–z`, `0–9`, `.`, `-`, `_`. Sie identifiziert das Paket über Versionen hinweg (§7).
- `pack.name`: Pflicht, nicht leer, höchstens 100 Zeichen. `pack.description`: optional, höchstens 1000 Zeichen. `pack.version`: Pflicht, nicht leerer Text, höchstens 50 Zeichen (Vorschlag: `MAJOR.MINOR.PATCH`; die App vergleicht Versionen nicht, sondern behandelt jede abweichende Version als „andere Version" derselben `pack.id`).
- `pack.homepage`: optional, **reiner Text**. Die App zeigt ihn an, macht ihn nicht zum Link, öffnet ihn nie und ruft ihn nie ab.
- `rules`: Liste, höchstens 1000 Einträge; mehr → abgelehnt. Jede Regel: `pattern_type` (`glob` | `regex` | `exact`), `pattern` (Text), `action` (`allow` | `confirm` | `deny`), `scope` (`{"type":"global"}` oder `{"type":"tag","tag":"<name>"}`), `priority` (ganze Zahl, Standard 0).
- Die Werte sind `snake_case`-Zeichenketten des Formats und **unabhängig von den internen Typnamen** der App; eine Umbenennung intern ändert das Format nicht. Ein Wert außerhalb der genannten Mengen macht diese Regel ungültig (§4.2), nicht die ganze Datei.
- **Unbekannte Felder werden ignoriert** (auf jeder Ebene) und beim Neu-Export nicht zurückgeschrieben.
- Datei größer als 2 MiB oder kein gültiges JSON → abgelehnt mit klarer Meldung; nichts wird gespeichert.

### 2.3 Was nie in einer Datei steht

Der Export schreibt **keine Geheimnisse, Server-IDs, Hostnamen, Regel-IDs, Zeitstempel oder sonstige lokale Daten**. Enthalten ist nur, was in den Feldern oben steht: die vom Nutzer in die ausgewählten Regeln getippten Muster und Schlagwortnamen sowie die von ihm eingegebenen Paketangaben. Der Export weist vor dem Speichern darauf hin, dass Muster Hostnamen, Pfade oder Benutzernamen enthalten können, wenn der Nutzer sie so geschrieben hat; die Prüfung dessen liegt beim Nutzer.

## 3. Export

1. **Ort:** Regelverwaltung (Spec 0009), Aktion „Exportieren…".
2. Der Nutzer wählt Regeln einzeln oder „alle exportierbaren". Exportierbar sind Regeln mit Geltungsbereich **global** oder **Schlagwort**.
3. **Server-Regeln sind nicht exportierbar** (Server-IDs sind lokal und nur auf dieser Maschine gültig). Der Dialog listet sie trotzdem auf, aber **ausgegraut und nicht auswählbar**, jeweils mit dem Grund. Darüber nennt er **die Zahl der nicht exportierbaren Regeln** („N Regeln mit Server-Bezug werden nicht exportiert") und den Grund. „Alle exportierbaren" wählt nur Regeln mit global/Schlagwort; die Zahl bezieht sich auf alle lokalen Regeln mit Server-Bezug. Es gibt keinen Weg, eine Server-Regel in die Auswahl zu bringen.
4. Der Nutzer gibt Name (Pflicht), Beschreibung, Version (Pflicht) und die Paket-ID an (Vorschlag aus dem Namen, änderbar; Regeln aus §2.2). Fehlt eine Pflichtangabe oder ist sie ungültig, ist „Speichern" gesperrt, mit Hinweis am Feld.
5. Gespeichert wird über den **System-Speichern-Dialog**. Der Export braucht **keine Netzwerkverbindung** und ruft nichts ab.
6. Ohne ausgewählte Regeln ist „Speichern" gesperrt. Ein Schreibfehler wird dem Nutzer gemeldet; es bleibt keine halbe Datei als Erfolg stehen.
7. Der Export ändert keine Regel.
8. Regeln aus einem importierten Paket (§7) lassen sich wie jede andere exportieren; die Herkunftsangabe wird dabei nicht mitgeschrieben.

## 4. Import: Lesen, Prüfen, Vorschau

### 4.1 Quelle

- Import erfolgt **nur aus einer lokalen Datei**, die der Nutzer über den System-Öffnen-Dialog wählt. **Es gibt keinen Download, keine URL-Eingabe, keinen Katalog und keinen Netzwerkaufruf**, auch nicht für `pack.homepage`.
- Gelesen wird die Datei vollständig in den Speicher (Grenze §2.2); sie wird nicht ausgeführt und nirgends als Pfad weiterverwendet.

### 4.2 Prüfung

Nach dem Parsen (§2.2) wird **jede Regel mit derselben Prüfung validiert wie beim Speichern einer Regel (Spec 0077)**: Glob- und Regex-Muster müssen sich übersetzen lassen (inklusive der Größenbegrenzung und beider Zweige pfadförmiger Globs, Spec 0060), das Muster darf nicht leer sein, Schlagwortname nicht leer. Eine Regel, die die Prüfung nicht besteht, ist **ungültig** und **nicht importierbar**; die Meldung der Prüfung wird in der Vorschau gezeigt. Eine ungültige Regel verhindert den Import der anderen nicht.

Eine Regel mit unbekanntem `pattern_type`, `action`, Scope-Typ oder Scope-Typ `server` (in einer Datei nicht vorgesehen) ist ebenfalls ungültig.

### 4.3 Vorschau

Vor jedem Speichern zeigt die App eine **Vorschau**; ohne Bestätigen wird nichts gespeichert. Die Vorschau zeigt Paketname, -ID, -Version, Beschreibung und Homepage (als Text) und **jede Regel** mit Aktion, Mustertyp, Muster, Geltungsbereich und Priorität. Zusätzlich markiert sie:

- **ungültig** (§4.2): nicht auswählbar, mit Grund;
- **Duplikat**: eine lokale Regel mit gleichem Mustertyp, gleichem Muster, gleicher Aktion und gleichem Geltungsbereich existiert bereits (die Priorität zählt nicht mit); wird übersprungen, nicht auswählbar;
- **Schlagwort unbekannt**: das Schlagwort einer Regel wird von keinem lokalen Server verwendet. Ein Hinweis, kein Hindernis: die Regel greift, sobald ein Server dieses Schlagwort bekommt.
- Doppelte Regeln innerhalb der Datei selbst gelten ab dem zweiten Vorkommen als Duplikat.

Die Vorschau berechnet nur; sie ändert nichts am Regelspeicher.

## 5. Allow-Regeln sind Opt-in

1. In der Vorschau sind **Deny- und Confirm-Regeln standardmäßig ausgewählt, Allow-Regeln nie.**
2. Jede Allow-Regel muss **einzeln** angehakt werden und trägt eine **sichtbare Warnung**, dass passende Kommandos danach **ohne Rückfrage** ausgeführt werden.
3. „Alle auswählen" **schließt Allow-Regeln nicht ein**; ebenso „Alle abwählen/auswählen" nach einer Filterung. Es gibt keine Aktion, die mehrere Allow-Regeln auf einmal auswählt.
4. Eine Vorschau mit mindestens einer ausgewählten Allow-Regel weist beim Bestätigen noch einmal auf deren Anzahl hin.
5. Dasselbe gilt unverändert für die erneute Importierung einer neueren Paketversion (§7.3, dort zusätzlich für das Entfernen von Deny-/Confirm-Regeln) und für Pakete, die über die Erweiterungsfassade (§9) kommen.
6. Eine Allow-Regel ändert die Auswertung nur in den Grenzen von Spec 0002: Die Hard-Blacklist und strengere Regeln bleiben davon unberührt.

## 6. Import: Ergebnis

- Ausgewählte Regeln werden **gewöhnliche lokale Regeln mit Herkunft `User`** (Spec 0037 §5) mit neuer ID; sie sind wie jede andere Regel bearbeit- und löschbar.
- Der Import **ändert oder löscht nie bestehende Regeln** und **berührt nie die Hard-Blacklist**.
- Der Import ist **alles oder nichts**: Alle ausgewählten Regeln und die Herkunftsangabe (§7) werden in **einer Transaktion** geschrieben. Scheitert irgendein Schritt, ist der Regelspeicher unverändert, und der Nutzer sieht einen Fehler.
- Danach meldet die App, **wie viele Regeln hinzugefügt, als Duplikat übersprungen und als ungültig abgewiesen** wurden. Vom Nutzer abgewählte Regeln zählen nicht dazu.
- Beim Anwenden wird die Prüfung (§4.2) und die Duplikaterkennung (§4.3) **erneut gegen den dann aktuellen Regelspeicher** durchgeführt, nicht der Vorschau vertraut; ein inzwischen entstandenes Duplikat wird übersprungen, eine nun ungültige Regel abgewiesen. Die ausgewählten Regeln werden anhand ihrer Position in der Paketliste benannt, nicht anhand von Inhalten, die das Frontend mitliefert.
- Die Regeln wirken sofort für neue Aktionen (derselbe Weg wie beim Speichern einer Regel in Spec 0009).

## 7. Herkunft (Provenienz)

### 7.1 Datenmodell

Neue Migration neben `0003_filter_rules.sql` (die Migration kommt mit der Umsetzung):

- Neue Tabelle `filter_rule_packs`: `pack_id TEXT PRIMARY KEY`, `name TEXT NOT NULL`, `version TEXT NOT NULL`, `imported_at TEXT NOT NULL`. Pro Paket-ID eine Zeile (die zuletzt importierte Version).
- Neue Spalte `filter_rules.pack_id TEXT NULL` (Verweis auf `filter_rule_packs.pack_id`, `ON DELETE`-Verhalten wird per Anwendungslogik gesteuert; NULL = selbst angelegt) und `filter_rules.pack_rule_key TEXT NULL`: ein stabiler Schlüssel der Regel im Paket, abgeleitet aus Mustertyp, Muster, Aktion und Geltungsbereich (also ohne Priorität), mit dem eine Regel bei einer neueren Version wiedererkannt wird. Index auf `pack_id`.
- Bestehende Zeilen behalten `NULL`; Rückwärtskompatibilität: eine Datenbank vor der Migration verhält sich wie vorher. Die Spalten ändern die Auswertung nicht.
- Die Herkunftsangabe ist reine Anzeige- und Verwaltungsinformation. Sie wird **nicht** exportiert (§3.8) und nicht an die KI gegeben.
- **Backup (Spec 0109):** Der Abschnitt „Filterregeln" des Voll-Backups hat typisierte, strenge Nutzdaten (0109 F10, U2) und trägt **keine** Herkunftsfelder; diese Spec erweitert ihn nicht und ändert Spec 0109 nicht. Nach einer Wiederherstellung sind alle Regeln selbst angelegte Regeln ohne `pack_id`; die Tabelle `filter_rule_packs` wird weder gesichert noch wiederhergestellt, und beim Ersetzen des Bestands (0109 I4, Schritt 3, innerhalb der Transaktion von I5) wird sie geleert, damit keine Paketzeile ohne Regeln bleibt. „Paket entfernen" gibt es danach für diese Regeln nicht mehr; ein erneuter Import desselben Pakets legt Regeln, die lokal schon gleich vorhanden sind, als Duplikate (§4.3) nicht an.
- Eine Regel, die der Nutzer nach dem Import **bearbeitet**, behält `pack_id` (Anzeige „aus Paket X, geändert"). Sie gilt bei einer Neu-Importierung (§7.3) als „vom Nutzer geändert" und wird nie überschrieben, sobald sich **irgendein** Feld ändert: Mustertyp, Muster, Aktion, Geltungsbereich **oder Priorität**. Auch eine reine Prioritätsänderung zählt.
- **Schlüssel bearbeiteter Regeln:** `pack_rule_key` wird beim Import gespeichert und beim Bearbeiten **nicht neu berechnet**; die Regel behält den ursprünglichen Schlüssel und bleibt so dem Paket zugeordnet. Enthält eine neuere Paketversion eine Regel mit diesem ursprünglichen Schlüssel (z. B. die Deny-Regel, die der Nutzer auf Allow geändert hat), zeigt die Vorschau sie **nicht** als „hinzugefügt" und nicht als „geändert", sondern nennt die lokale Regel als „vom Nutzer geändert"; es gibt keinen zweiten Eintrag für denselben Schlüssel, und die bearbeitete Regel bleibt unverändert. Eine Regel „hinzugefügt" (Deny/Confirm vorausgewählt, §5) gibt es nur, wenn lokal **keine** Regel mit diesem Schlüssel existiert. Fehlt der Schlüssel in der neuen Version, gilt die bearbeitete Regel nicht als „entfernt" und wird nicht gelöscht.

### 7.2 Anzeige und Entfernen

- Die Regelverwaltung zeigt bei Regeln aus einem Paket **Paketname und -version** (und die Paket-ID im Tooltip bzw. in der Detailansicht).
- Aktion **„Paket entfernen"**: entfernt **alle Regeln dieses Pakets in einem Schritt**, nach Bestätigung, die Paketname, Version und die Anzahl der Regeln nennt. Selbst angelegte Regeln und Regeln aus anderen Paketen bleiben unberührt; die Zeile in `filter_rule_packs` wird mit entfernt. Auch das geschieht in einer Transaktion. Einzelne Regeln eines Pakets lassen sich weiterhin einzeln löschen.
- Wird die letzte Regel eines Pakets einzeln gelöscht, wird auch dessen Paketzeile entfernt.

### 7.3 Neuere Version desselben Pakets

- Wird ein Paket mit einer `pack.id` importiert, die lokal schon Regeln hat, zeigt die Vorschau **statt der einfachen Liste einen Vergleich** gegenüber den noch vorhandenen Regeln der älteren Version (§7.1 Schlüssel):
  - **hinzugefügt:** Regel im neuen Paket ohne Gegenstück lokal (neuer Schlüssel);
  - **geändert:** gleicher Schlüssel, andere Priorität (die einzigen Felder, die sich bei gleichem Schlüssel unterscheiden können);
  - **entfernt:** lokale Regel aus dem alten Paket, die im neuen Paket fehlt und noch unverändert vorhanden ist;
  - **unverändert** (gleicher Schlüssel und gleiche Priorität): nur gezählt;
  - **vom Nutzer geändert:** lokal bearbeitete Regeln eines Pakets (§7.1), auch bei reiner Prioritätsänderung, werden nie überschrieben, auch nicht durch einen „geändert"-Eintrag des Pakets für denselben Schlüssel; die Vorschau nennt sie (ein Schlüssel erscheint nur in einer Kategorie).
- Es wird **nicht gespeichert, welche Regeln der Nutzer gelöscht hat** (kein Grabstein). Eine Regel des neuen Pakets, die lokal nicht mehr vorhanden ist, gilt daher als **hinzugefügt** und folgt §5, auch wenn der Nutzer sie früher gelöscht hat. Sie wird nie stillschweigend angelegt, sondern wie jede neue Regel in der Vorschau gezeigt und nur bei Auswahl angelegt.
- Auswahl: „hinzugefügt" folgt §5 (Deny/Confirm vorausgewählt, **Allow nie**). Ein „geändert" bei einer Allow-Regel ist ebenfalls nicht vorausgewählt.
- **Entfernen kann erweitern, was ohne Rückfrage läuft:** Fällt eine Deny- oder Confirm-Regel weg, kann ein passendes Allow (Vorrang Deny > Confirm > Allow, Spec 0002 §3.2) greifen. Deshalb ist eine „entfernt"-Regel nur vorausgewählt, wenn sie eine **Allow**-Regel ist. **Entfernen einer Deny- oder Confirm-Regel ist Opt-in je Regel**: nicht vorausgewählt, mit Warnung („danach laufen passende Kommandos womöglich ohne Rückfrage oder werden nicht mehr blockiert"), kein „Alle auswählen" für sie, und der Bestätigungsdialog nennt ihre Anzahl wie bei §5.4. Bleibt eine solche Regel abgewählt, bleibt sie lokal bestehen (mit ihrem `pack_id`); die Paketzeile erhält trotzdem die neue Version.
- Angewendet wird alles in **einer Transaktion** (§6); die Paketzeile erhält die neue Version.
- Ein Paket mit anderer `pack.id` ist ein anderes Paket, auch bei gleichem Namen.
- Dieselbe Version nochmals importieren zeigt denselben Vergleich (alles „unverändert", daher „nichts zu tun").

## 8. Community-Pakete

Pakete der Community sind **gewöhnliche Paketdateien**, die jeder veröffentlichen kann (z. B. in einem öffentlichen Repository mit Regelpaketen). Die App hat in dieser Spec **keinen eingebauten Katalog** und lädt nichts herunter. Vertrauen in ein Paket entsteht durch die Vorschau und die Allow-Opt-in-Regeln (§5), nicht durch eine Quelle.

## 9. Erweiterungspunkt

- Vorschau und Anwenden sind über die **öffentliche Erweiterungsfassade** (`app_logic::ext`, Spec 0038) erreichbar, **mit dem Paketinhalt als Eingabe** (die Bytes oder der geparste Text der Datei, nicht ein Pfad und nicht eine URL). Eine Erweiterung kann so Pakete aus eigener Quelle anbieten und durchläuft dennoch **dieselbe** Prüfung (§2, §4.2), **dieselbe** Vorschau (§4.3, §7.3) und **dasselbe** Allow-Opt-in (§5) und dieselbe Transaktion (§6).
- Zwei Schritte: (1) `Vorschau(Paketinhalt) → Vorschau-Ergebnis` ohne Seiteneffekt; (2) `Anwenden(Paketinhalt, Auswahl) → Ergebnis`. Die Auswahl nennt Regeln über ihre Position; Allow-Regeln werden nur übernommen, wenn die Auswahl sie ausdrücklich nennt. Die Fassade bietet keinen Weg, die Prüfung, die Vorschau oder das Opt-in zu überspringen oder Regeln mit anderer Herkunft als `User` anzulegen.
- **Die Community-Edition registriert keine solche Quelle.** Der Standardweg (Datei öffnen, §4) ist unverändert. Ein Test belegt, dass ohne Erweiterung kein zusätzliches Verhalten entsteht.
- Kein neuer Platz in der Oberfläche der Regelverwaltung für Paketlisten einer Erweiterung: Eine Erweiterung ruft die Fassade aus ihrer eigenen Oberfläche auf. Wird ein Platz gebraucht, ist das eine eigene Änderung.
- Änderungen der Fassade gehen, wie in Spec 0038 beschrieben, nur zusammen mit einem Changelog-Eintrag und einer Anpassung von Spec 0038.

## 10. Sicherheitszusagen

1. Der Import legt nur Regeln mit Herkunft `User` an; `Builtin`/`Organization` (Spec 0037 §5) sind nicht über Dateien erzeugbar.
2. Die Hard-Blacklist, die Vorrangreihenfolge und `FilterEngine::evaluate` sind unverändert; Tests aus Spec 0002 laufen ohne Änderung.
3. Kein Netzwerkaufruf beim Import oder Export; `pack.homepage` wird nie geöffnet oder abgerufen.
4. Allow-Regeln aus einer Datei werden nie ohne ausdrückliche, einzelne Auswahl angelegt; weder „alle auswählen" noch die Erweiterungsfassade umgehen das.
5. Ein Fehler irgendwo im Import hinterlässt keine teilweise angelegten Regeln.
6. Paketinhalt (Name, Beschreibung, Muster) wird in der Oberfläche als **Text** dargestellt, nie als HTML oder Markdown, und gelangt nicht ungeprüft in Logs oder KI-Kontext.
7. `THREAT-MODEL.md` Abschnitt 6 wird mit der Umsetzung ergänzt: Eine importierte Allow-Regel ist eine Entscheidung des Nutzers, getroffen in der Vorschau.

## 11. Testfälle

1. **Unbekannte Formatversion:** Datei mit `format_version: 2` (und mit `0`, Text statt Zahl) wird abgelehnt, Meldung nennt gefundene und unterstützte Version, Regelspeicher unverändert. Falsches `format` → „kein Regelpaket".
2. **Ungültiges Muster:** Paket mit Glob `ls [abc *` und Regex `a{1000}{1000}` → beide ungültig und nicht auswählbar, Grund angezeigt; die anderen Regeln bleiben importierbar; Ergebnis meldet die Zahl abgewiesener Regeln.
3. **Duplikat:** Lokal existiert eine Regel gleichen Mustertyps, Musters, Aktion und Geltungsbereichs (andere Priorität) → als Duplikat markiert und übersprungen; ein zweites Vorkommen innerhalb der Datei ebenso; Ergebnis zählt „übersprungen".
4. **Server-Bereich beim Export:** Lokal 5 Regeln, davon 2 mit Server-Bereich → der Dialog zeigt die 2 ausgegraut und nicht auswählbar und nennt „2 nicht exportiert" mit Grund; „alle exportierbaren" wählt die 3 anderen; die Datei enthält 3 Regeln, keine Server-ID, keinen Hostnamen, keine Regel-ID. Server-Regeln lassen sich auch über die Kommando-Schnittstelle nicht in den Export bringen (abgewiesen).
5. **Allow nicht vorausgewählt:** Vorschau mit Allow-, Confirm-, Deny-Regel → Confirm und Deny ausgewählt, Allow nicht und mit Warnung; „Alle auswählen" lässt Allow unberührt; ohne einzelnes Anhaken wird keine Allow-Regel angelegt (auch über die Erweiterungsfassade).
6. **Transaktion zurückrollen:** Schlägt das Schreiben der dritten von fünf ausgewählten Regeln fehl (eingespielter Fehler), sind weder Regeln noch Paketzeile gespeichert; es erscheint ein Fehler.
7. **Herkunft entfernen:** Zwei Pakete A (3 Regeln) und B (2 Regeln) und eine eigene Regel → „Paket entfernen" für A löscht genau 3 Regeln und die Paketzeile A; B und die eigene Regel bleiben. Bestätigung nennt Name, Version, Anzahl.
8. **Neuere Version:** Paket v1 importiert, danach v2 mit einer neuen, einer in der Priorität geänderten und einer fehlenden Regel → Vorschau zeigt 1 hinzugefügt, 1 geändert, 1 entfernt; eine neue Allow-Regel ist nicht vorausgewählt; nach Anwenden stimmen Regeln und Version; eine vom Nutzer bearbeitete Regel wird nicht überschrieben. Fälle: (a) lokal nur die Priorität einer Paketregel geändert, v2 enthält für denselben Schlüssel eine andere Priorität → Vorschau nennt die Regel als „vom Nutzer geändert", kein „geändert"-Eintrag, Priorität bleibt nach Anwenden die des Nutzers; (b) lokal Deny auf Allow geändert, v2 enthält weiterhin die ursprüngliche Deny-Regel → kein „hinzugefügt"-Eintrag, die lokale Regel behält `pack_id` und ursprünglichen Schlüssel, bleibt Allow und wird als „vom Nutzer geändert" genannt; wird dagegen die Regel lokal gelöscht, erscheint die Deny-Regel als „hinzugefügt" und ist vorausgewählt. Eine in v1 vorhandene, lokal gelöschte Regel erscheint bei v2 als „hinzugefügt" (bei Allow nicht vorausgewählt). Eine entfernte Allow-Regel ist vorausgewählt; eine entfernte **Deny- oder Confirm-Regel** nicht (mit Warnung), ohne Anhaken bleibt sie lokal bestehen. Gegenprobe: entfernte Deny-Regel plus passendes lokales Allow → ohne Opt-in bleibt `FilterEngine::evaluate` bei Deny.
9. **Runde:** Export und anschließender Import der Datei in einen leeren Speicher liefert gleiche Regeln (Mustertyp, Muster, Aktion, Geltungsbereich, Priorität), Herkunft `User`, `pack_id` gesetzt.
10. **Unbekannte Felder:** Zusätzliche Felder auf jeder Ebene werden ignoriert, der Import gelingt.
11. **Kein Netz:** Import und Export laufen bei gesperrtem Netzwerk; eine `pack.homepage` löst keinen Aufruf aus.
12. **Auswertung unverändert:** Mit importierten Regeln liefert `FilterEngine::evaluate` dieselben Ergebnisse wie mit gleichen von Hand angelegten Regeln; die Hard-Blacklist gewinnt weiterhin gegen eine importierte Allow-Regel.
13. **Erweiterungsfassade:** Vorschau und Anwenden mit Paketinhalt durchlaufen dieselbe Prüfung und dasselbe Opt-in; ohne registrierte Quelle ändert sich der Standardweg nicht.
14. **Grenzen:** mehr als 1000 Regeln oder Datei über 2 MiB → abgelehnt; Pflichtfelder fehlen → abgelehnt.
15. **Backup und Herkunft:** Voll-Backup (Spec 0109) einer Installation mit Paketregeln, Wiederherstellung auf leerer Installation → Regeln gleich, `pack_id` leer, keine Paketzeile; das Backup-Format enthält kein Herkunftsfeld.

## 12. Umsetzung in Schritten

Vorschlag für Folge-Issues ohne Überschneidung:

1. **Format und Export.** Pakettypen, Parser/Schreiber und Format-Prüfung in `crates/core/src/filter/` (z. B. `rule_pack.rs`), Export-Logik in `crates/app-logic/src/filter_rules.rs`, Kommandos in `crates/app-shell/src/commands/rules.rs`, Export-Dialog in `frontend/src/components/filterRules/`. Testfälle 1 (Format), 4, 9, 10, 14.
2. **Import und Vorschau.** Vorschau-Berechnung (ungültig, Duplikat, unbekanntes Schlagwort), Anwenden in einer Transaktion, Allow-Opt-in, Import-Dialog; `crates/app-logic/src/` (neues Modul neben `filter_rules.rs`), `crates/persistence-sqlite` (Transaktion), `crates/app-shell/src/commands/rules.rs`, Frontend `filterRules/`. Testfälle 2, 3, 5, 6, 11, 12.
3. **Herkunft und neuere Version.** Migration `filter_rule_packs` und Spalten (§7.1), Anzeige, „Paket entfernen", Versionsvergleich; `crates/persistence-sqlite/migrations/`, `crates/persistence-sqlite/src/`, `crates/app-logic/src/`, Frontend. Außerdem: Backup-Import leert `filter_rule_packs` (§7.1, Backup). Testfälle 7, 8, 15.
4. **Erweiterungsfassade.** Zwei Funktionen in `crates/app-logic/src/ext.rs`, Anpassung von Spec 0038 und Changelog-Eintrag, `THREAT-MODEL.md` Abschnitt 6. Testfall 13.
