# ADR 0117 — Web-Recherche über die serverseitigen Werkzeuge des Providers

Status: akzeptiert
Betrifft: Spec 0105, Issue #162

## Kontext

Issue #162 legt fest: Web-Recherche nur über die serverseitigen
Web-Werkzeuge des Providers, Default an, abschaltbar, Webinhalt als nicht
vertrauenswürdig behandeln. Offen ließ das Issue unter anderem, welche
OpenAI-kompatiblen Endpunkte Web-Recherche bekommen, ob die
Injection-Prüfung (ADR 0027) auch Webinhalt prüft, welche Werkzeug-Versionen
genutzt werden und wie Webinhalt im Verlauf gespeichert wird.

## Entscheidungen

1. **Nur Anthropic.** Die OpenAI-kompatible Familie bekommt kein
   Web-Werkzeug (Option a aus dem Issue). Der Client spricht die
   Chat-Completions-API; ob ein beliebiger kompatibler Endpunkt
   `web_search_options` kennt, lässt sich nicht sicher feststellen, und ein
   unbekanntes Feld kann eine Anfrage scheitern lassen. Unterstützung für
   bekannte OpenAI-Modelle oder den Wechsel auf die Responses-API ist eine
   eigene Folgeaufgabe.
2. **Basis-Versionen der Werkzeuge** (`web_search_20250305`,
   `web_fetch_20250910`) statt der Versionen mit „dynamic filtering". Die
   Basis-Versionen funktionieren mit allen unterstützten Claude-Modellen und
   erzeugen keine zusätzlichen Code-Ausführungs-Blöcke im Stream. Je Anfrage
   `max_uses` 5 je Werkzeug, Seitenabruf mit `max_content_tokens` 25 000 und
   eingeschalteten Zitaten (für die Quellenanzeige). Keine
   `allowed_domains`/`blocked_domains`.
3. **Nur der Haupt-Chat.** Nebenaufrufe teilen sich die Provider-Instanz
   mit dem Haupt-Chat. Sie setzen immer einen eigenen `max_tokens`-Hinweis
   (Spec 0065), der Haupt-Chat nie; daran erkennt der Provider den
   Haupt-Chat. Damit braucht der Anfragekontext kein neues Feld.
4. **Feste Werkzeug-Reihenfolge: Web-Werkzeuge vor den Aktions-Werkzeugen.**
   Der Cache-Breakpoint (Spec 0064) bleibt auf dem letzten
   Aktions-Werkzeug und deckt damit den ganzen, je Provider und
   Einstellung konstanten Satz ab; serverseitige Werkzeuge tragen selbst
   keinen Breakpoint.
5. **Eigene Nachrichtenart statt Text.** Eine Recherche wird strukturiert
   gespeichert (Art, Anfrage/URL, Treffer, zitierte Quellen, Seitentext,
   Fehlercode). Dafür braucht `chat_messages.content_type` den neuen Wert
   `web_activity` (Migration 0020, Tabellen-Neuaufbau wie Migration 0009,
   weil SQLite eine `CHECK`-Constraint nicht ändern kann). Der Schalter ist
   eine eigene Spalte mit `DEFAULT TRUE` (Migration 0019). Vorteil:
   Anzeige beim Fortsetzen ohne Text-Parsing, Fencing erst beim Senden an
   einer einzigen Stelle, Redaction je Feld.
6. **Gespeichert mit der Rolle „Assistent".** So zählt eine URL aus einer
   früheren Recherche in späteren Anfragen als Teil der KI-Ausgabe. Die
   Abruf-Regel des Providers (nur URLs, die schon im Gespräch stehen) wird
   dadurch eher strenger als im nativen Verlauf, nie lockerer.
7. **Seitentext gekürzt auf 20 000 Zeichen.** Der Verlauf geht bei jeder
   Anfrage erneut an den Provider; ein ungekürzter Seitentext würde ihn
   unverhältnismäßig aufblähen. Die Kürzung ist im gespeicherten Eintrag
   markiert, wird der KI im Fence mitgeteilt und auf der Karte angezeigt.
8. **Verlauf ohne die Original-Blöcke des Providers.** Spätere Anfragen
   schicken die Recherche als gefencten Text, nicht die Original-Blöcke mit
   den verschlüsselten Suchtreffern. Die Treffer-Ausschnitte einer Websuche
   sind damit in späteren Zügen nicht mehr im Kontext, nur Titel, URLs und
   Seitentext. Dafür gilt das eigene Fencing und die eigene Redaction für
   alles, was zurückgeht, und der Verlauf bleibt providerunabhängig.
9. **Injection-Prüfung beim Speichern (Option a).** Sie läuft über
   Treffertitel und Seitentext, sobald eine Recherche gespeichert wird.
   Die Treffer-Ausschnitte liefert der Provider nur verschlüsselt; sie sind
   nicht prüfbar. Zusätzlich setzt jede Recherche das Flag „nicht
   vertrauenswürdigen Inhalt gelesen" (Spec 0039, Abschnitt 5).
10. **Freigabe erst am Antwortende.** Der Provider hält Recherchen wie
    Aktionsvorschläge bis zum Ende der Antwort zurück, ordnet dann die
    Zitate zu und gibt sie vor den Aktionsvorschlägen frei. Wird die Antwort
    wegen eines abgeschnittenen Aktionsvorschlags verworfen (Spec 0065),
    verfallen auch ihre Recherchen. Die bestehende Liste der als vollständig
    geltenden Stop-Gründe bleibt unverändert.
11. **`pause_turn` wird nicht automatisch fortgesetzt.** Eine pausierte
    Antwort endet wie eine normale. Automatisches Fortsetzen bräuchte die
    Original-Blöcke des Providers (Punkt 8).

## Konsequenzen

**Positiv:**
- Keine neue Verbindung und keine neue Abhängigkeit; der Netzwerk-Test
  bleibt unverändert.
- Webinhalt durchläuft dieselben Schutzschichten wie gelesener
  Serverinhalt (Fencing, Redaction, Eskalation, Injection-Prüfung).

**Negativ / Trade-off:**
- Hat der Betreiber des Anthropic-Kontos die Websuche abgeschaltet, scheitert
  jede Chat-Anfrage mit einem Fehler des Providers, bis der Nutzer die
  Einstellung ausschaltet. Ein automatischer Wiederholversuch ohne
  Web-Werkzeuge ist nicht eingebaut.
- Spätere Züge sehen von einer Websuche nur Titel und URLs, nicht die
  Treffer-Ausschnitte (Punkt 8).
- Eine ältere App-Version kann einen Verlauf mit Recherche-Einträgen nicht
  mehr laden.
