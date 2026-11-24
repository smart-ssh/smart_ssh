# ADR 0145 — Zwischenablage als dritte Freigabequelle für Uploads

## Status

Accepted

Betrifft: ADR 0113, Spec 0020 (5.2), Spec 0054 (Teil 3), Issue #571

## Problem

„Einfügen“ lokaler Dateien aus der Zwischenablage soll hochladen. Eine
Freigabe darf nach ADR 0113 nur durch etwas entstehen, das die Oberfläche
nicht allein auslösen kann. Ein `invoke` aus dem Webview belegt keine
Nutzergeste.

## Entscheidung

- Der Webview nennt nie einen lokalen Pfad. `clipboard_has_files` liefert nur
  einen Wahrheitswert (Menüeintrag ja/nein).
- `claim_clipboard_files` liest die Dateiverweise im Backend und zeigt einen
  nativen Bestätigungsdialog mit Dateiliste und Zielordner. Nur die Bestätigung
  im Dialog gilt als Nutzergeste: erst dann werden die Pfade der Sitzung
  freigegeben und zurückgegeben. Abbrechen, ein verschwundener Dialog oder eine
  beendete Sitzung geben nichts frei. Der Zielordner ist reiner Anzeigetext.
- Die Zwischenablage ist damit eine dritte Quelle neben Öffnen-Dialog und
  nativem Ablegen. Der anschließende Upload ist unverändert.
- Nicht lesbare Zwischenablage: kein Menüeintrag, kein Dialog, kein
  Ersatzweg. Es werden nur absolute, existierende Pfade übernommen;
  `file://`-Verweise werden dekodiert.
