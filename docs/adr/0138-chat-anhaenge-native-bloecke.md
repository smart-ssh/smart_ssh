# ADR 0138 — Chat-Anhänge als native Inhaltsblöcke, Fähigkeitstabelle

## Status

Accepted · Issue: #470 · Spec 0006, Spec 0110

## Kontext

Seit #469 tragen Nachrichten Anhänge, doch Provider rendern Bild/PDF nur als Platzhalterzeile. Das Issue verlangt echte Blöcke und eine Prüfung, ob das Modell sie lesen kann; offen blieb, woher die Fähigkeitsdaten kommen.

## Entscheidungen

1. **Statische Tabelle je Anbieter mit konservativem Standard** (`ai-providers/src/content.rs`). Die Modellerkennung (`discover_models`) liefert heute nur Modell-IDs, keine Modalitäten; ein Überschreiben durch Discovery ist deshalb nicht umgesetzt (bewusste Verkleinerung gegenüber der Empfehlung im Issue). Es kann ergänzt werden, sobald eine API Modalitäten meldet.
2. **Unbekannte Modelle:** Anthropic und der offizielle OpenAI-Endpunkt erlauben sie (nur bekannte Textmodelle sind gesperrt). Jeder andere Endpunkt gilt als „nicht unterstützt", außer der Modellname enthält einen Bildmodell-Hinweis; PDFs gibt es dort nie. Der Endpunkt wird am Host erkannt, nicht per Teilstring.
3. **Ablehnung im Provider (`send`)**, vor dem Request-Aufbau, als `AiError::AttachmentUnsupported` (seit #506 `AiError::InputUnsupported(MediaClass)`; Code `AI_ATTACHMENT_UNSUPPORTED`). Geprüft wird der gesamte Verlauf, nicht nur die letzte Nachricht: ein stilles Weglassen älterer Bilder würde dem Modell einen falschen Kontext geben. Preis: Nach einem Modellwechsel blockiert ein früherer Bildanhang weitere Nachrichten, bis ein neuer Chat beginnt.
4. **Nachrichteninhalt als typisierte Teile** (`ContentPart`): Text bleibt der bereits redigierte/gefencete String; Bild/PDF-Bytes folgen als Block direkt hinter ihrem gefencten `local_file`-Text. Reine Textnachrichten behalten die String-Form. *Geändert durch ADR 0141, Entscheidung 5 (#506): `ContentPart::Binary { mime, filename, base64 }` ersetzt `Image`/`Pdf`; die Blockart folgt der Medienklasse, die Fähigkeitsprüfung ist nach Medienklasse geschlüsselt (`InputSupport::accepts`).*
5. **Token-Schätzung unverändert** (fester Wert je Binäranhang aus #469); sie zählt die Blöcke bereits.

## Konsequenzen

Bilder/PDFs erreichen unterstützte Modelle nativ; die Community-Oberfläche hat weiterhin keinen Auswahlweg (Edition-Auflöser). Ein Modell, das fälschlich als „kann Bilder" gilt, scheitert beim Anbieter mit dessen Fehler.
