# ADR 0123 — Fehlergrenze nur für Erweiterungen des Erststart-Hinweises

Status: akzeptiert
Betrifft: Issue #159, Spec 0031 (Abschnitt 6), Spec 0045 (Abschnitt 9), Issue #157

## Problem

Bei der Einführung der Erweiterungen des Erststart-Hinweises (Issue #157)
wurde bewusst auf eine Fehlergrenze verzichtet: Ein Render-Fehler sei
Sache des Erweiterungsautors, wie bei den übrigen Registry-Typen. Das
Frontend hat keine React-Fehlergrenze. Ein Wurf beim Rendern einer
Erweiterung hängt deshalb den ganzen Baum aus, und mit ihm den
Pflicht-Hinweis, ohne dessen Bestätigung keine erste Verbindung möglich
ist (Spec 0031, Abschnitt 4).

## Entscheidung

1. **Je Erweiterung eine eigene Fehlergrenze** (kleine lokale
   Klassenkomponente im Hinweis, keine neue Abhängigkeit). Sie umschließt
   den ganzen Bereich der Erweiterung; nach einem Fehler rendert sie gar
   nichts, auch nicht den Rahmen.
2. **Protokoll** per `console.error` mit der Kennung der Erweiterung, im
   gleichen Stil wie der bestehende Fehler-Log der Weiter-Handler.
3. **Handler verwerfen.** Ein bereits registrierter Weiter-Handler der
   gescheiterten Erweiterung wird entfernt, spätere Registrierungen unter
   ihrer Kennung werden ignoriert. Eine Erweiterung, deren Darstellung der
   Nutzer nicht (mehr) gesehen hat, soll nichts auf Grundlage dieser
   Darstellung auslösen.
4. **Nur dieser Slot.** Einstellungs-Abschnitte und Dokument-Aktionen
   behalten das bisherige Verhalten. Nur der Erststart-Hinweis steht vor
   einem Pflichtschritt; eine gemeinsame Fehlergrenze für alle Renderer
   wäre eine eigene, breitere Änderung mit Anpassung von Spec 0045.
5. **Keine Änderung an der Registry-Schnittstelle** (Spec 0045,
   Abschnitt 9).

## Konsequenzen

- Der Pflicht-Hinweis bleibt bestätigbar, egal wie sich eine Erweiterung
  verhält, solange sie beim Rendern scheitert. Endlosschleifen oder
  blockierende Effekte fängt eine Fehlergrenze nicht ab.
- Asynchrone Fehler (z. B. in Event-Handlern oder Promises einer
  Erweiterung) erreichen die Fehlergrenze nicht; sie hängen den Baum aber
  auch nicht aus.
