# Spec 0064 — Prompt-Caching

Status: umgesetzt
Zweck: Der Request an den KI-Anbieter ist so aufgebaut, dass sein stabiles
Präfix gecacht wird. Das senkt Kosten und die Rate-Limit-Last, weil bei den
meisten Modellen gecachte Input-Tokens nicht gegen das Input-Limit zählen.
Bezüge: Spec 0006, Spec 0057 (Kompaktierung), Spec 0061 (Rate-Limit-
Drosselung), Spec 0039 (Fencing), Spec 0062 (Abbruchgrund-Logging).

## Grundsatz

Der Cache ist ein **Präfix-Match**: Jede Änderung im Präfix invalidiert
alles danach. Maßgeblich ist deshalb die Cache-Disziplin (Reihenfolge und
Stabilität), nicht nur das Setzen von Cache-Markierungen.

## Teil 1: Aufbau nach Änderungshäufigkeit

Selten Veränderliches steht vorn, häufig Veränderliches hinten:

1. System-Prompt (ändert sich praktisch nie),
2. Werkzeug-Definitionen (stabil, solange der Satz konstant bleibt),
3. Server-Notiz bzw. Server-Kontext (ändert sich selten, ist aber groß —
   der Hauptgewinn),
4. Gesprächsverlauf (wächst),
5. neue Nachricht bzw. neues Kommando-Ergebnis.

## Teil 2: Cache-Killer vermeiden

- **Kein Datum, keine Uhrzeit und kein sitzungsspezifisches Banner
  (`uname`) im System-Prompt.** Solche Zustandsangaben stehen als erste,
  nie persistierte Nachricht im Verlauf, nach dem stabilen Präfix.
- Der Werkzeug-Satz bleibt innerhalb einer Sitzung konstant.
- Innerhalb einer Aufrufkette wechselt das Modell nicht (jedes Modell hat
  seinen eigenen Cache). Zweitmeinung, Auto-Titel u. ä. laufen als eigene
  Aufrufe mit eigenem Cache.
- Alles, was sich pro Anfrage ändert und vor dem Verlauf steht, ist ein
  Cache-Killer und gehört nicht ins Präfix.

## Teil 3: Cache-Markierungen

Der Anthropic-Request markiert das Ende des stabilen Präfixes (System-Prompt
und Werkzeuge) mit einem Cache-Breakpoint. Ein leerer System-Text erhält
keine Markierung (der Anbieter lehnt das ab). Kurze Prompts unter der
modellabhängigen Mindestlänge profitieren nicht; sie schaden aber auch
nicht.

## Teil 4: Andere Anbieter

OpenAI cacht automatisch ohne Markierung, und die Reihenfolge nach Teil 1
hilft dort ebenfalls (Präfix-Match). Andere OpenAI-kompatible Anbieter und
lokale Modelle (Ollama) bleiben unberührt und werden nicht gestört.

## Teil 5: Messung

Die Nutzungsangaben jeder Anthropic-Antwort (gelesene und geschriebene
Cache-Tokens, normale Input-Tokens) werden geloggt, ohne Inhalt. So ist die
Trefferquote sichtbar und prüfbar, ob etwas das Präfix bricht.

## Teil 6: Wechselwirkung mit der Kompaktierung

Die Kompaktierung (Spec 0057) ändert den Gesprächsverlauf und invalidiert
den Cache ab der Änderungsstelle. Weil der Verlauf hinten steht, bleibt das
stabile Präfix gecacht. Wird die Server-Notiz verkürzt gesendet, ändert sich
dagegen ein Präfix-Teil und der Rest ist ein Cache-Miss; das ist ein
bewusst hingenommener Konflikt zwischen Notiz-Kürzung und Caching.

## Sicherheitszusagen

- Fencing und Redaction bleiben unverändert; nur Reihenfolge und Struktur
  des Requests ändern sich, nicht was redigiert oder gefenced wird. Der
  Notiz-Fence (Spec 0039) bleibt.
- Das Cache-Logging enthält nur Token-Zahlen, kein Secret.
- Kompaktierung und Drosselung (Spec 0057/0061) bleiben funktionsfähig.

## Akzeptanzfälle

- Stabiles Präfix steht vorn, Volatiles hinten.
- Kein Datum und kein Banner im System-Prompt.
- Die Cache-Markierung ist gesetzt, die Nutzungsangaben werden geloggt.
- Andere Anbieter funktionieren unverändert.
- Fencing und Redaction sind unverändert.
