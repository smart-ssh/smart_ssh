# Spec 0006 — KI-Anbieter-Abstraktion

Status: umgesetzt
Zweck: Einheitliche Schnittstelle, über die die App mit einer frei wählbaren
KI chattet, Kommando- und Notizvorschläge strukturiert entgegennimmt und
Kommando-Ergebnisse für den nächsten Gesprächsschritt zurückgibt.
Bezüge: Spec 0003 (Notizen, Vorschlagsarten), Spec 0005 (Kommando-Ergebnis),
Spec 0002 (Filter-Engine: jeder Kommandovorschlag läuft dort durch).

## 1. Ziel

Die App spricht mit OpenAI, Anthropic, lokalen Modellen (z. B. Ollama) und
beliebigen OpenAI-kompatiblen Endpunkten über dieselbe Abstraktion. Der
Anbieter liefert Chat-Text und strukturierte Vorschläge (Kommando, Notiz-
Änderung); die App spielt Kommando-Ergebnisse für den nächsten Schritt
zurück.

**Abgrenzung:** Die KI schlägt nur vor. Sie führt nichts aus und umgeht nie
die Filter-Engine — jeder Kommandovorschlag durchläuft unverändert die
Präzedenz-Kette aus Spec 0002, unabhängig davon, welcher Anbieter ihn
erzeugt hat.

## 2. Aufteilung

Die Abstraktion (Datentypen, Fehlerarten, Schutzschichten) ist unabhängig
von konkreten Anbietern; Anbieter sind austauschbar und in Tests durch einen
Mock ersetzbar. OpenAI, generische OpenAI-kompatible Endpunkte und Ollama
(OpenAI-kompatibler Modus) teilen sich eine Implementierung, weil sie
dasselbe Anfrage-/Antwortschema nutzen. Anthropic hat wegen des
abweichenden Tool-Formats eine eigene.

## 3. Gesprächsmodell

- Eine Antwort kommt als **Strom** von Ereignissen: Text-Häppchen (erscheinen
  wortweise im Chat), strukturierte Vorschläge, Ende oder Fehler.
  Strukturierte Vorschläge sind eigene Ereignisse und werden nicht aus
  Freitext geparst (außer im Fallback, Abschnitt 4).
- Der Kontext einer Anfrage besteht aus dem System-Kontext (wirksame Notizen
  aus Spec 0003 plus OS-/Distro-Information), dem Verlauf und der Liste der
  erlaubten Vorschlagsarten.
- Verlaufsnachrichten haben eine Rolle (Nutzer, Assistent, Aktionsergebnis).
  Ein Aktionsergebnis trägt Kommando und Kommando-Ergebnis (Spec 0005).

## 4. Vorschläge: Tool-Calling und Fallback

- **Natives Tool-Calling** (OpenAI, Anthropic, die meisten aktuellen
  Modelle): Die erlaubten Vorschlagsarten werden 1:1 auf das Tool-Format des
  Anbieters abgebildet, zurückgelieferte Tool-Calls werden direkt zu
  Vorschlägen. Das ist der bevorzugte, zuverlässigere Weg.
- **Fallback** für Modelle ohne zuverlässiges Tool-Calling (z. B. manche
  lokalen Modelle): Das Modell wird angewiesen, Vorschläge als klar
  abgegrenzten JSON-Block in die Antwort zu schreiben, der geparst wird.
  **Scheitert das Parsen, gilt der Text als normale Chat-Nachricht, nicht als
  Fehler.** Die KI hat dann schlicht keinen strukturierten Vorschlag
  gemacht; sicherheitsrelevant ist das nicht, weil ohne Filter-Engine nichts
  ausgeführt wird.
- Welche Kategorie gilt, stellt der Nutzer je Anbieter-Konfiguration ein
  (Schalter „unterstützt natives Tool-Calling"); sie wird nicht automatisch
  erkannt, weil eine Erkennung still das falsche Verhalten wählen könnte.

## 5. Redaction vor dem Versand

Jedes Kommando-Ergebnis läuft durch die Redaction, **bevor** es in den
Kontext der nächsten Anfrage aufgenommen wird. Die Standard-Redaction
erkennt gängige Muster (Private-Key-Blöcke, `password=`/`token=`/
`api_key=`-artige Zeilen, AWS-Schlüssel u. ä.) und ersetzt sie durch
`[REDACTED]`. Nutzer können eigene Muster ergänzen.

Redaction läuft **immer**, unabhängig vom Anbieter und auch bei lokalen
Modellen: Konsistenz ist wichtiger als die Annahme, ein lokales Modell sei
„sicherer". Die Details der Redaction beschreiben Spec 0016 und Spec 0039.

## 6. Fehlerarten

Anbieterfehler werden auf feste Arten abgebildet und nie still verschluckt:
Authentifizierung fehlgeschlagen, Rate-Limit, Netzwerkfehler, ungültige
Antwort, Kontext zu groß, Anbieter nicht erreichbar. Ein ungültiger API-Key
erscheint als Authentifizierungsfehler, nicht als leere Antwort.

Bei „Kontext zu groß" kürzt die aufrufende Seite ältere Verlaufseinträge,
bevor erneut gesendet wird (Spec 0057, Spec 0087).

## 7. Akzeptanzfälle

- Tool-Call-Antwort eines nativen Anbieters wird korrekt zu einem
  Vorschlag.
- Wohlgeformter Fallback-JSON-Block wird korrekt geparst.
- Fehlerhafter Fallback-JSON-Block wird zu Text statt zu einem Absturz.
- Redaction maskiert bekannte Secret-Muster in Kommando-Ergebnissen und
  lässt den übrigen Output unverändert.
- Ein ungültiger API-Key wird als Authentifizierungsfehler durchgereicht.

## 8. Konfiguration und Grenzen

- Die Anbieter-Konfiguration (aktiver Anbieter, Modell, Basis-URL für
  generische Endpunkte) wird gespeichert; der API-Key selbst liegt nie in
  der Datenbank, sondern im Credential-Speicher (Spec 0003, Spec 0101).
- Es ist ein Anbieter je Sitzung aktiv; ein Wechsel pro Nachricht ist nicht
  vorgesehen.
- Die Kontext-Kürzung arbeitet mit einer Näherung, nicht
  mit einem anbieterspezifischen Tokenizer; Aufbau und Kürzung des Kontexts beschreibt Spec 0057.
