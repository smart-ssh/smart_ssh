# Spec: Dateimanager-Layout — verstellbare Spalten & Bereiche

Status: Entwurf
Repo: **öffentlich** `smart_ssh`, Frontend
Modul: SFTP-Dateimanager-UI, das Layout, das KI-Bereich und SSH-/SFTP-Bereich
teilt
Abhängigkeiten: SFTP-Browser (0020), Frontend-Design-Konventionen

> Reine **Layout-/UX-Verbesserungen** am Dateimanager — kein
> server-verändernder Zugriff, keine Filter-Engine-Berührung, **Priorität
> NORMAL**. Das UI fühlt sich noch starr an; der Nutzer soll das Layout an
> seine Arbeitsweise anpassen können. Die chmod-/Rechte-Bearbeitung ist
> **bewusst NICHT Teil dieser Spec** (eigener, sicherheitsrelevanter Schritt,
> weil server-verändernd — bleibt im Backlog).

## Teil 1: Verstellbare Spaltenbreiten im Dateimanager

Die Dateiliste hat feste Spaltenbreiten (Name, Größe, Datum, Rechte, …). Der
Nutzer soll die Spalten in der Breite **ziehen** können.

- Zwischen den Spaltenköpfen ein **Drag-Handle**; Ziehen ändert die Breite
  der Spalte links davon.
- Sinnvolle **Mindestbreiten** pro Spalte (nicht auf 0 zusammenziehbar).
- Die **Name-Spalte** sollte den flexiblen Rest einnehmen (sie ist am
  wichtigsten und variabelsten), die anderen (Größe/Datum/Rechte) eher fix,
  aber ebenfalls verstellbar.
- **Persistenz**: Die gewählten Breiten überleben einen Neustart (im
  bestehenden Frontend-Settings-/State-Mechanismus ablegen — dort, wo
  ähnliche UI-Präferenzen schon liegen; **kein** localStorage in Artefakten,
  aber die App ist kein Artefakt — der reguläre Persistenz-Weg der App ist
  gemeint). Falls es noch keinen Ort für UI-Präferenzen gibt, den saubersten
  wählen und mir beschreiben.

## Teil 2: Verstellbare Bereichsgröße (KI-Bereich ↔ SSH-/SFTP-Bereich)

Die Aufteilung zwischen dem **KI-Bereich** und dem **SSH-/SFTP-Bereich**
(bzw. Terminal/Dateimanager) ist fix. Der Nutzer soll den **Splitter**
dazwischen ziehen können, um mehr Platz für das eine oder andere zu geben.

- Ein **Drag-Divider** zwischen den beiden Bereichen (horizontal oder
  vertikal, je nach aktuellem Layout).
- Sinnvolle **Mindestgrößen** für beide Bereiche (keiner auf 0 ziehbar).
- **Persistenz** wie bei Teil 1 (die gewählte Aufteilung überlebt Neustart).
- Auf **kleinen Fenstern** darf der Splitter das Layout nicht unbrauchbar
  machen — die Mindestgrößen greifen, notfalls Fallback auf die
  Standardaufteilung.

## Nicht Teil dieser Spec

- **chmod / Rechte-Bearbeitung** — server-verändernd, eigene
  sicherheitsrelevante Spec (muss durch die Bestätigungs-/Filter-Logik).
  Bleibt im Backlog.
- Keine Änderung an der Datei-Logik selbst (Upload/Download/Anzeige) — nur
  Layout.

## Design/Konventionen

- Halte dich an den **frontend-design-Skill** (Design-Tokens, keine
  Ad-hoc-Styles).
- Die Drag-Handles/Divider sollen sich an bestehende UI-Muster anlehnen
  (falls es schon irgendwo verstellbare Elemente gibt, dasselbe Muster
  nutzen).
- Barrierearm: Divider/Handles per Tastatur bedienbar wäre schön (nicht
  zwingend), mindestens aber ein klarer Hover-/Cursor-Hinweis
  (`col-resize`/`row-resize`).

## Testbarkeit

- Spaltenbreite ziehen → Breite ändert sich, Mindestbreite wird nicht
  unterschritten, Wert überlebt Neustart.
- Bereichs-Splitter ziehen → Aufteilung ändert sich, Mindestgrößen greifen,
  Wert überlebt Neustart.
- Kleines Fenster → Layout bleibt bedienbar (Mindestgrößen).
- Bestehende Dateimanager-Funktionen (Navigation, Upload/Download) unverändert.

## Reihenfolge

1. Spaltenbreiten (Teil 1) — abgegrenzter, kleiner.
2. Bereichs-Splitter (Teil 2).
Beide teilen sich den Persistenz-Mechanismus für UI-Präferenzen.
