# Spec 0058 — Notiz-Editor: Hinweis bei großen Notizen, Fokus, Localhost ohne Port

Status: umgesetzt
Zweck: Kleine Verhaltensregeln rund um den Notiz-Editor und die
Serveranzeige: ein Hinweis bei großen Notizen, der Sprung ins Notizfeld
aus dem Kürzungs-Vorschlag und die Anzeige des lokalen Pseudo-Servers.
Bezüge: Spec 0003 (Notizen), Spec 0032 (Localhost als Sitzung), Spec 0057
(Kürzungs-Vorschlag beim Verbindungsende), Spec 0079 (Schwelle, Karte
„Notiz ist sehr groß"), ADR 0050.

## Teil 1: Hinweis beim Bearbeiten großer Notizen

- Erreicht der Text im Notiz-Editor die Schwelle für große Notizen
  (10 000 Zeichen, Spec 0079, A4), erscheint über dem Editor ein dezenter,
  nicht blockierender Hinweis: „Diese Notiz ist sehr groß und kann bei
  langen Sitzungen für den KI-Kontext gekürzt werden. Die gespeicherte
  Notiz bleibt vollständig erhalten."
- Der Hinweis gilt für Server- und Gruppen-Notizen und für die Notiz des
  lokalen Pseudo-Servers. Er reagiert auf den aktuell eingegebenen Text,
  nicht erst auf den gespeicherten.
- Er ist **rein informativ**: Er verhindert das Speichern nicht und
  verändert die Notiz nicht.
- Bei Server-Notizen (auch beim lokalen Pseudo-Server) enthält er den Link
  „Jetzt zusammenfassen". Er startet dieselbe KI-Kürzung wie „Ja,
  zusammenfassen" auf der Karte beim Verbindungsende (Spec 0057); das
  Ergebnis erscheint als Notiz-Vorschlag mit Vorschau und muss bestätigt
  werden. Schlägt die Anfrage fehl, steht die Fehlermeldung im Hinweis.
  Gruppen-Notizen haben diesen Link nicht.
- Karte und Hinweis nutzen dieselbe Schwelle aus einer Quelle.

## Teil 2: „Mache ich selbst" und der lokale Pseudo-Server

- „Mache ich selbst" auf der Karte „Notiz ist sehr groß" öffnet die
  Notiz des Servers, scrollt zum Notizfeld und setzt den Fokus hinein,
  damit der Nutzer direkt bearbeiten kann. Das geschieht einmal beim
  Öffnen, nicht bei jeder späteren Aktualisierung der Ansicht.
- Der lokale Pseudo-Server hat eine eigene Notiz. Die Karte „Notiz ist
  sehr groß" und „Mache ich selbst" funktionieren für ihn genauso wie für
  jeden anderen Server.

## Teil 3: Lokaler Pseudo-Server ohne Port

Der lokale Pseudo-Server verbindet sich nicht über das Netz und hat keinen
Port. Die Serverliste zeigt für ihn nur Nutzer und Host, ohne Port; echte
Server zeigen `Nutzer@Host:Port`.
