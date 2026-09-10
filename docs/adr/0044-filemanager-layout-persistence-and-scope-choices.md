# 0044-filemanager-layout-persistence-and-scope-choices

## Status
Akzeptiert

## Kontext

Spec 0053 (Dateimanager-Layout — verstellbare Spalten & Bereiche) ließ
Teil 1 ausdrücklich offen, wo verstellbare UI-Präferenzen (Spaltenbreiten,
Bereichsaufteilung) gespeichert werden sollen: "Falls es noch keinen Ort
für UI-Präferenzen gibt, den saubersten wählen **und mir beschreiben**."
Außerdem ließ Abschnitt "Design/Konventionen" die Tastaturbedienbarkeit
der Drag-Handles ausdrücklich offen ("wäre schön (nicht zwingend)").

## Entscheidung

**1. Persistenz: derselbe `tauri-plugin-store`-Ablageort (`settings.json`)
wie Sprache (`i18n.ts`) und Risiko-Zweitmeinung (`riskSettings.ts`), in
einer neuen Datei `layoutSettings.ts`.**

Es gab bereits einen Ort für genau diese Klasse von Einstellung — eine
reine, nicht sicherheitsrelevante UI-Präferenz ohne Server-/Session-Bezug
und ohne Rust-seitigen Leser. `i18n.ts`s `settingsStore()`-Singleton wird
wiederverwendet statt einer zweiten `load("settings.json", ...)`-Stelle.
Zwei neue Schlüssel: `fileManagerColumnWidths` (Objekt mit
Größe/Rechte/Geändert-Breiten — Name ist nie enthalten, sie ist immer der
flexible Rest, s. `FileBrowserPanel.tsx`) und `aiSshSplitWidthPx` (die
Breite des rechten/SSH-Bereichs in Pixeln). Beide Werte sind **global**,
nicht pro Server/Session — die App merkt sich, wie der Nutzer sein UI
eingerichtet hat, nicht wie jeder einzelne Server aussehen soll.

Geladene Werte werden validiert (endliche, positive Zahl innerhalb einer
plausiblen Obergrenze von 10 000px) statt roh übernommen — ein von Hand
editiertes oder durch eine künftige Formatänderung unpassend gewordenes
`settings.json` darf nie zu `NaN`/negativen/absurd großen Breiten führen;
ungültige/fehlende Felder fallen still auf die eingebauten Defaults
zurück.

**2. Kein Tastatur-Zugriff auf die Drag-Handles — nur Maus-/Touch-Drag mit
Hover-/Cursor-Hinweis (`cursor-col-resize`/`cursor-row-resize`).**

Die Spec verlangt das explizit nicht ("nicht zwingend"), nur den
Hover-/Cursor-Hinweis als Minimum. Eine vollständige Tastaturbedienung
(Pfeiltasten zum schrittweisen Verstellen, `tabindex`, `aria-valuenow`
etc.) wäre für ein reines Layout-/UX-Feature mit NORMAL-Priorität
unverhältnismäßiger Zusatzaufwand gegenüber dem Nutzen gewesen — die
Spaltenbreiten/Bereichsaufteilung sind Komfort-Feineinstellungen, keine
für die Kernfunktion (Navigation, Upload/Download) notwendige Bedienung,
die ohne Maus vollständig unzugänglich bliebe.

Direkte Konsequenz: die Handles selbst tragen kein `role="separator"`
mehr (ein Spec-Reviewer-Fund korrigierte eine erste Fassung, die das
für die Spalten-Handles gesetzt hatte) — ein benanntes, aber nicht
fokussierbares/tastaturbedienbares ARIA-Element hätte Screenreadern beim
Durchlaufen der Tabellenkopfzeile ein funktionsloses Element angesagt.
Stattdessen `aria-hidden="true"` (rein visuelle Maus-Affordanz) plus
`data-testid` als reiner Test-Anker. Der Bereichs-Splitter in
`SessionView.tsx` behält `role="separator"` (er trennt tatsächlich zwei
Inhaltsregionen voneinander, unabhängig von Tastaturbedienbarkeit ist das
semantisch zutreffend).

## Konsequenzen

- Ein künftiges Feature, das Spaltenbreiten/Bereichsaufteilung auch per
  Tastatur verstellbar machen soll, müsste `ColumnResizeHandle`/den
  Bereichs-Splitter in `SessionView.tsx` um `tabIndex`,
  Tastatur-Eventhandler und passende ARIA-Attribute (`role="separator"`
  mit `aria-valuenow`/`aria-valuemin`/`aria-valuemax`) erweitern — aktuell
  nicht vorhanden.
- Mehrere offene Tabs teilen sich denselben globalen Wert, synchronisieren
  sich aber nicht live: ein Drag in Tab A schreibt in `settings.json`,
  ein bereits gemounteter Tab B behält bis zu seinem nächsten Neu-Mount
  seinen alten Wert (`FileBrowserPanel`/`SessionView` laden nur einmal
  beim Mounten). Bewusste Vereinfachung für dieses NORMAL-Priorität-
  Feature — kein Cross-Tab-Sync-Mechanismus eingeführt.
