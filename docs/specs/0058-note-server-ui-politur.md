# Spec: Notiz- & Server-UI-Politur (Session-Modell Etappe 5 + Reste)

Status: Entwurf
Repo: **öffentlich** `smart_ssh`, Frontend + etwas Core
Abhängigkeiten: Session-Modell Etappe 4 (Notiz-Kürzungs-Dialog), Notizen
(0003), Localhost-Pseudo-Server (0031 o. Ä.)

> Kohärentes, **risikoarmes** Politur-Paket rund um **Notizen und die
> Server-Form** — thematisch zusammengehörig, gleicher Code-Bereich.
> **Priorität NORMAL** (UI-Politur, keine Sicherheitslogik). Mehrere kleine
> Verbesserungen, je eigener Commit.

## Teil 1: Hinweis beim Bearbeiten großer Notizen (Session-Modell Etappe 5)

Das Gegenstück zum Sitzungsende-Kürzungs-Dialog (Etappe 4): ein **proaktiver,
sanfter Hinweis** schon **beim Bearbeiten** einer ungewöhnlich großen Notiz —
damit der Nutzer es merkt, bevor es in einer langen Sitzung zum Problem wird.

- Im **Notiz-Editor**: Ist die Notiz über dem Schwellwert (dieselbe Konstante
  wie Etappe 4, `LARGE_NOTE_DIALOG_THRESHOLD_BYTES` = 8000 — **wiederverwenden**,
  keine zweite Konstante), einen **dezenten Hinweis** anzeigen (Banner/
  Inline-Hinweis, kein blockierender Dialog): „Diese Notiz ist sehr groß und
  kann bei langen Sitzungen für den KI-Kontext gekürzt werden. Die
  gespeicherte Notiz bleibt vollständig erhalten."
- **Rein informativ** — kein Zwang, keine Aktion erzwungen. Der Nutzer *darf*
  eine große Notiz haben; der Hinweis erklärt nur die Konsequenz.
- Optional (wenn einfach): ein Link/Button „jetzt zusammenfassen", der
  denselben KI-Kürzungs-Fluss wie Etappe 4 auslöst (mit Diff-Bestätigung) —
  aber nur wenn das ohne viel Zusatzaufwand geht, sonst nur der Hinweis.

## Teil 2: Etappe-4-Reste (aus dem Etappe-4-Review, bewusst offen gelassen)

Zwei kleine Nacharbeiten am Sitzungsende-Notiz-Dialog:
- **„Mache ich selbst" fokussiert das Notizfeld.** Aktuell öffnet die Option
  das Server-Formular, aber ohne Scroll/Fokus aufs Notizfeld — der Nutzer
  muss es erst suchen. Beim Öffnen zum Notizfeld scrollen und es fokussieren,
  damit man direkt loslegen kann.
- **Lokaler Pseudo-Server bekommt den Dialog nie** (keine `servers`-Zeile,
  aus der eine Notizgröße gelesen werden könnte). Prüfen: Kann der
  Pseudo-Server überhaupt eine (große) Notiz haben? Falls ja, den Dialog auch
  für ihn ermöglichen; falls nein (strukturell keine Notiz), ist das korrekt
  und wird nur **dokumentiert** (kein Fix nötig). Beschreibe mir, was zutrifft.

## Teil 3: Lokaler Pseudo-Server zeigt Port 0 an

Der Localhost-Pseudo-Server zeigt in der UI **Port 0** an (ein Platzhalter/
Nicht-Wert, der für den Pseudo-Server keinen Sinn ergibt). Statt „0" entweder
**gar keinen Port** anzeigen oder einen sinnvollen Hinweis („lokal", kein
Port). Rein kosmetisch, aber „Port 0" wirkt wie ein Bug. Beschreibe mir, wo
das herkommt und wie du es sauber löst (kein Port-Feld für den Pseudo-Server
vs. spezielle Anzeige).

## Nicht Teil dieser Spec
- Settings-Registry Notify-Mechanismus (eigenes Architektur-Thema, bleibt im
  Backlog).
- UI-Feedback bei nicht antwortendem Provider (~20min-Stille — braucht ein
  neues Chat-Event, eigenes Thema).

## Design/Konventionen
- **frontend-design-Skill** beachten (Tokens, keine Ad-hoc-Styles).
- Teil 1: der Hinweis fügt sich dezent in den Notiz-Editor ein (nicht
  aufdringlich, kein blockierender Dialog).
- Schwellwert-Konstante aus Etappe 4 **wiederverwenden** (eine Quelle).

## Testbarkeit
- Teil 1: Hinweis erscheint bei großer Notiz im Editor, nicht bei normaler;
  gespeicherte Notiz unberührt (rein informativ).
- Teil 2: „Mache ich selbst" → Notizfeld fokussiert/gescrollt; Pseudo-Server-
  Notiz-Verhalten geklärt.
- Teil 3: Pseudo-Server zeigt nicht mehr „Port 0"; echte Server zeigen ihren
  Port normal.

## Reihenfolge
1. Teil 3 (Port 0) — kleinster, isoliert.
2. Teil 2 (Etappe-4-Reste) — im gerade gebauten Etappe-4-Code.
3. Teil 1 (Etappe-5-Hinweis) — der Kern, nutzt die Etappe-4-Konstante.
