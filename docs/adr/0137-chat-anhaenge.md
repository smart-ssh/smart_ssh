# ADR 0137 — Chat-Anhänge: eigene Nachrichtenvariante und Auflöser

## Status

Superseded by 0141 · Issue: #469 · Spec 0110

## Kontext

Der Kern soll Dateianhänge an Nutzer-Nachrichten tragen, speichern und sicher senden, ohne selbst Dateien auszuwählen. Das Issue lässt offen, ob Anhänge eine neue `MessageContent`-Variante oder ein Feld der Text-Nachricht sind.

## Entscheidungen

1. **Eigene Variante `MessageContent::Attachments`, nach dem Text gespeichert.** Altes gespeichertes JSON bleibt ohne Migration lesbar (die Variante kommt dort nie vor); nur die `CHECK`-Constraint auf `content_type` wird erweitert (Migration 0021). Preis: Eine Kompaktierungsrunde beginnt bei jeder Nutzer-Rolle, der Anhang bildet also eine eigene Runde neben dem Text. Eingereihte Nachrichten verhalten sich schon so.
2. **Inhalt als Base64 im JSON.** Bytes als JSON-Zahlenliste wären ~4× größer. Text wird als UTF-8-Bytes im selben Feld geführt; beim Annehmen wird UTF-8 geprüft.
3. **Redaction in zwei Stufen.** Vor dem Speichern und bei jedem Senden (`reapply_redaction_for_send`) wird der rohe Textinhalt redigiert; gefencet wird erst beim Request-Aufbau (`ChatAttachment::to_model_text`), wie bei `WebActivity`. Bild/PDF-Bytes werden nie interpretiert.
4. **Auflöser liefert `Option`.** Mehrere Editionen/Quellen können registriert sein; `None` heißt „nicht meine ID". Die Grenzen (5 Anhänge, 1 MiB) prüft der Kern am Ergebnis, nie der Auflöser.
5. **Anhänge werden nicht eingereiht.** Läuft ein Turn, wird eine Nachricht mit Anhängen abgelehnt (`ATTACHMENTS_WHILE_BUSY`); die Warteschlange trägt nur Text. Ein Umbau der Warteschlange wäre ein eigenes Issue.
6. **Wiederaufgenommener Verlauf** zeigt Anhänge vorerst als Textzeile mit Dateinamen (kein neuer Frontend-Typ); die Anzeige liefert die Edition-UI.
7. **`Feature::ChatAttachments` ist nur Vokabular.** Der Kern prüft die Berechtigung nicht; die Edition, die einen Auflöser registriert, entscheidet.
8. **`serde_json` als Dev-Abhängigkeit von `core`** (nur für Tests, schon im Lockfile).

## Konsequenzen

Die Community Edition ändert sich nicht. Provider rendern Bild/PDF bis zu einem Folge-Issue nur als Platzhalterzeile.
