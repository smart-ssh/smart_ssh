# 0037-document-actions-render-only-on-document-card

## Status
Akzeptiert

## Kontext

Spec 0045, Abschnitt 3 beschreibt den Andockpunkt `registerDocumentAction`
als sitzend "dort, wo im Dokument-/**Chat-Bereich** der Markdown-Export
sitzt (0012)" — mehrdeutig zwischen zwei existierenden Stellen im
`ChatPanel`:

- `DocumentCard` (Spec 0012): die eigene Karte für ein `chat-document-
  generated`-Ereignis (die KI erzeugt bewusst ein "Dokument" statt einer
  normalen Antwort), mit einem "Als Markdown speichern"-Button.
- `AssistantMessageView`: JEDE normale Assistenten-Antwort im Chat trägt
  ebenfalls einen (kleineren) "📄 Als Markdown"-Export-Button — ein
  separater Code-Pfad, nicht Teil des Dokument-Konzepts aus Spec 0012.

Ein unabhängiger `spec-reviewer`-Review der Implementierung merkte an,
dass Abschnitt 4 der Spec die Mehrdeutigkeit zwar auflöst ("die
Dokument-Karten-Komponente aus 0012" — eindeutig `DocumentCard`), aber
diese Scope-Entscheidung nirgends explizit im Code/Commit festgehalten
war, obwohl sie eine sichtbare Nutzerkonsequenz hat: Eine registrierte
Dokument-Aktion (z. B. der private Word-Export) erscheint an der
Dokument-Karte, aber NICHT am kleineren Export-Button jeder normalen
Chat-Antwort, obwohl beide strukturell denselben `onExport(content,
title, format)`-Aufruf teilen.

## Entscheidung

`registerDocumentAction`-Einträge werden ausschließlich in `DocumentCard`
gerendert (Spec 0045, Abschnitt 4, wörtlich: "die Dokument-Karten-
Komponente aus 0012"), NICHT in `AssistantMessageView`. Begründung:

- Das ist die im Spec-Text tatsächlich benannte Stelle, keine freie
  Interpretation der mehrdeutigen Einleitung aus Abschnitt 3.
- `DocumentCard` ist der einzige Ort, an dem die KI ein Ergebnis
  ausdrücklich ALS Dokument markiert (`ProposeDocument`/`chat-document-
  generated`, Spec 0012) — der begriffliche Rahmen, den "Dokument-Aktion"
  benennt. Jede Chat-Antwort pauschal als "Dokument" zu behandeln würde
  den Andockpunkt entwerten (jede Antwort bekäme dieselben Aktionen wie
  ein bewusst erzeugtes Dokument).
- Kleinerer, klarer beobachtbarer Scope ist leichter nachträglich zu
  erweitern (auf `AssistantMessageView`) als vorschnell überall
  einzubauen und später wieder einzuschränken.

Falls sich zeigt, dass ein Feature (z. B. der private Word-Export) die
Aktion auch am normalen Antwort-Export-Button braucht: das ist eine
bewusste, spätere Erweiterung dieses Andockpunkts auf einen zweiten
Renderer, kein impliziter Fix dieses Schritts.

## Konsequenzen

- Eine registrierte Dokument-Aktion ist für den Nutzer nur an
  KI-generierten Dokumenten (Spec 0012) sichtbar, nicht an jeder
  Chat-Antwort — dokumentiertes, bewusstes Verhalten, kein Bug.
- Falls ein künftiges Feature beide Stellen braucht, ist das eine explizit
  zu treffende Erweiterung (neuer Renderer in `AssistantMessageView`),
  keine Ausweitung des bestehenden Andockpunkts ohne Spec-/ADR-Update.
