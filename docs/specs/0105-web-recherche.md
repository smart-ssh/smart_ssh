# Spec 0105 — Web-Recherche der KI über den Provider

Status: umgesetzt · Issue: #162, #169, #168
Zweck: Die KI kann beim Antworten im Web suchen und Webseiten lesen (nur
Text), um aktuelle Dokumentation, Fehlermeldungen und Release Notes
einzubeziehen. Die Recherche läuft ausschließlich über die serverseitigen
Web-Werkzeuge des KI-Providers; die App öffnet dafür keine eigene
Verbindung.
Review-Priorität: ERHÖHT (nicht vertrauenswürdiger Inhalt im KI-Kontext,
Fencing, Injection-Prüfung)

Bezüge: Spec 0006 (KI-Provider), Spec 0024 (Übersetzungen), Spec 0039
(Fencing, Eskalation nach dem Einlesen), Spec 0064 (Prompt-Caching),
ADR 0027 (Injection-Prüfung), ADR 0117 (Entscheidungen zu dieser Spec),
ADR 0124 (Websuche im Provider-Konto abgeschaltet),
ADR 0127 (Web-Recherche über die Responses-API von OpenAI).

## 1. Welche Provider

- **Anthropic:** Web-Recherche verfügbar (Websuche und Seitenabruf des
  Providers), sofern „Natives Tool-Calling" eingeschaltet ist.
- **OpenAI (offizieller Dienst):** Web-Recherche verfügbar (Websuche des
  Providers), sofern „Natives Tool-Calling" eingeschaltet ist. Anders als
  bei Anthropic gibt es keinen Seitenabruf: Der Provider liefert keinen
  Seitentext zurück.
- **Generische OpenAI-kompatible Endpunkte, Ollama und OpenAI mit
  eigener Basis-URL:** keine Web-Recherche. Die Anfrage enthält kein
  Web-Werkzeug, und der Schalter erscheint nicht; der Chat verhält sich
  genau wie ohne diese Spec.

## 2. Einstellung

- Jeder Provider, der Web-Recherche unterstützt, hat in den KI-Einstellungen
  den Schalter **„Web-Recherche (im Web suchen und Seiten lesen)"** — im
  Formular beim Anlegen und in der Liste der eingerichteten Provider. Bei
  anderen Providern erscheint der Schalter nicht.
- **Default: an**, für neu angelegte und für bereits bestehende Provider.
- Der Wert wird gespeichert und überdauert einen Neustart.
- Eine Änderung gilt für Sitzungen, die danach verbunden werden; eine
  laufende Sitzung behält den Stand, mit dem sie verbunden wurde.
- Ausgeschaltet enthält keine Anfrage ein Web-Werkzeug.

## 3. Was die KI darf

- Nur im Haupt-Chat. Automatische Nebenaufrufe (Risiko-Zweitmeinung,
  Injection-Prüfung, Sitzungstitel, Notiz-Vorschlag und -Kürzung,
  Verlaufs-Verdichtung) bekommen nie ein Web-Werkzeug.
- Je KI-Anfrage höchstens 5 Websuchen und 5 Seitenabrufe. Ein Seitenabruf
  liefert höchstens etwa 25 000 Tokens Inhalt. Bei OpenAI gilt die Grenze
  von 5 Werkzeugaufrufen je Anfrage (Websuchen); Seitenabrufe gibt es dort
  nicht.
- Keine Bestätigung je Suche oder Abruf.
- Nur Text: Suchtreffer und Seitentext. Bilder, Downloads und als
  Binärdaten gelieferte Dokumente (z. B. PDF) übernimmt die App nicht.
- Eine Seite kann nur abgerufen werden, wenn ihre Adresse schon im Gespräch
  vorkommt (Regel des Providers). Die App lockert diese Regel nicht und
  schränkt keine Domains frei.
- Als im Gespräch genannt zählt auch eine Adresse, die in der Ausgabe eines
  ausgeführten Befehls oder in einem anderen Aktionsergebnis (z. B. gelesener
  Dateiinhalt) steht: Solche Ergebnisse gehen als Nutzertext an den Provider,
  die Adresse ist deshalb ab der nächsten Anfrage abrufbar. Nicht nur der
  Nutzer kann also abrufbare Adressen einbringen — auch eine eingeschleuste
  Anweisung kann einen Befehl vorschlagen, der eine Adresse ausgibt. Das
  begrenzen die bestehenden Schichten: Der erzeugende Befehl läuft durch
  Filter, Risiko-Einschätzung und Bestätigung (Abschnitt 5), und nach dem
  Einlesen seiner Ausgabe gilt die Sitzung bereits als „nicht
  vertrauenswürdigen Inhalt gelesen" (Spec 0039, Abschnitt 5).
- Der angebotene Werkzeugsatz hängt nur vom Provider und der Einstellung ab,
  nicht von Sitzung, Server oder Verlauf (Spec 0064 bleibt wirksam).
  Einzige Ausnahme: Hat das Provider-Konto die Web-Werkzeuge abgelehnt
  (Abschnitt 7), bietet die Sitzung ab da keine mehr an.

## 4. Anzeige im Chat

- Jede Websuche und jeder Seitenabruf erscheint als eigene Karte nach dem
  bis dahin gestreamten Antworttext:
  - Websuche: „Websuche: ‹Suchanfrage›", Seitenabruf: „Webseite gelesen:
    ‹URL›".
  - Die von der KI zitierten Quellen mit Titel und URL. Ohne Zitat: die
    Zahl der Treffer bzw. „Seite gelesen, nicht zitiert".
  - Quellen sind Text, keine Links — ein Klick öffnet nichts.
  - Wurde der Seitentext für den Verlauf gekürzt (Abschnitt 6), steht das
    als Hinweis auf der Karte.
- Bei OpenAI zeigt die Karte die Suchanfrage und die von der Antwort
  zitierten Quellen (Titel, URL); der Provider nennt nicht, welche Quelle
  zu welcher Suche gehört, die Zitate stehen deshalb bei der letzten Suche
  der Antwort. Es gibt keine Karte „Webseite gelesen": Öffnet der Provider
  eine Seite, zeigt die App das als Suchkarte mit der Adresse der Seite,
  ohne Seitentext, und die Sitzung gilt als „nicht vertrauenswürdigen
  Inhalt gelesen" (Abschnitt 6). Zitiert eine Antwort Quellen, ohne dass
  eine Suche gemeldet wurde, zeigt die App eine Karte mit diesen Quellen.
- Meldet der Provider einen Fehler des Web-Werkzeugs (z. B. Seite nicht
  erreichbar, Höchstzahl erreicht, Adresse nicht im Gespräch), zeigt die
  Karte einen übersetzten, lesbaren Hinweis; ein unbekannter Fehlercode
  erscheint in einem allgemeinen Hinweis. Die Antwort läuft weiter und
  endet normal.
- Eine fortgesetzte Sitzung zeigt die Karten an derselben Stelle wieder.
- Alle Texte gibt es in jeder unterstützten Sprache (Spec 0024).

## 5. Keine Ausführung

- Eine Web-Recherche ist reine Information. Sie löst keinen
  Aktionsvorschlag, keine Filter-Prüfung, keine Bestätigung und keine
  automatische Folgerunde aus.
- Befehle auf dem Server — auch `curl`/`wget` — laufen unverändert durch
  Filter und Risiko-Einschätzung.

## 6. Nicht vertrauenswürdiger Inhalt

- Suchanfrage, URLs, Treffertitel und Seitentext gelten als nicht
  vertrauenswürdig (Spec 0039).
- **Speichern:** Jede Recherche wird als eigene Nachricht im Chat-Verlauf
  gespeichert (verschlüsselt wie jede andere Nachricht). Der Seitentext wird
  dabei auf 20 000 Zeichen gekürzt; die Kürzung ist markiert, nie still.
- **Redaction:** Alle Textfelder laufen vor dem Speichern, vor der Anzeige
  und vor jedem erneuten Senden durch die Redaction — auch die
  Suchanfrage, in die die KI ein Geheimnis eingesetzt haben könnte.
- **Fencing:** Geht eine gespeicherte Recherche in einer späteren Anfrage
  zurück an die KI (Verlauf, Verdichtung, Fortsetzen), dann nur innerhalb
  eines `web_content`-Fence mit dem Hinweis, dass es sich um Daten aus dem
  offenen Web handelt, nie um Anweisungen. Fence-Marker im Inhalt (auch im
  Titel) werden unschädlich gemacht und können den Fence nicht schließen.
- **Rolle:** Eine Recherche geht als Teil der KI-Antwort zurück, nicht als
  Nutzereingabe. Eine URL aus einem früheren Suchtreffer oder Seitentext
  zählt damit für die Abruf-Regel des Providers nicht als vom Nutzer
  genannt. Das gilt nur für Recherche-Inhalt; eine Adresse in der Ausgabe
  eines Befehls oder einem anderen Aktionsergebnis ist dagegen abrufbar
  (Abschnitt 3).
- **Eskalation nach dem Einlesen:** Nach der ersten Web-Recherche gilt die
  Sitzung als „nicht vertrauenswürdigen Inhalt gelesen" (Spec 0039,
  Abschnitt 5): Die Eskalationsstufe des Servers greift für die folgenden
  Aktionen. Eine fortgesetzte Sitzung mit Recherche im Verlauf startet
  ebenso.
- **Sofort beim Eintreffen:** Die Sitzung gilt schon in dem Moment als
  „nicht vertrauenswürdigen Inhalt gelesen", in dem ein Ergebnis eines
  Web-Werkzeugs (auch ein Werkzeug-Fehler) eintrifft — nicht erst am Ende
  der Antwort. Das gilt auch, wenn der Nutzer die Antwort danach stoppt oder
  sie mit einem Fehler (Verbindungsabbruch, Zeitüberschreitung, Fehler des
  Providers) endet: Bis dahin gestreamter Text bleibt im Verlauf, und die
  folgenden Aktionen werden wie nach einer Recherche eskaliert. Eine
  Recherchekarte erscheint in diesen Fällen nicht. Eine Antwort ohne
  Web-Ergebnis eskaliert nie.
- **Injection-Prüfung:** Ist die Prüfung auf eingeschleuste Anweisungen
  eingerichtet, läuft sie beim Speichern einer Recherche über Treffertitel
  und Seitentext. Ein Verdacht eskaliert die nächste Aktion wie bei
  gelesenen Serverinhalten. Bei OpenAI läuft sie über die Titel der
  zitierten Quellen. Die Treffer-Ausschnitte einer Websuche liefert
  der Provider nur verschlüsselt; sie kann die App nicht prüfen.

## 7. Grenzfälle

- **Pausierte Antwort:** Unterbricht der Provider eine lange Recherche
  (Pause statt Abschluss), endet die Antwort dort wie eine normale Antwort.
  Der Nutzer kann mit einer neuen Nachricht weitermachen lassen.
- **Websuche in der Organisation abgeschaltet:** Hat der Betreiber des
  Provider-Kontos die Websuche abgeschaltet, lehnt der Provider jede Anfrage
  mit Web-Werkzeug ab. Erkannt wird nur genau diese Ablehnung (Anfrage mit
  Web-Werkzeug, vom Provider als ungültige Anfrage abgelehnt mit dem
  Hinweis, dass die Websuche nicht aktiviert ist). Dann:
  - Der Chat zeigt einen übersetzten Hinweis: Web-Recherche ist für dieses
    Provider-Konto nicht verfügbar, die Sitzung antwortet ohne; dauerhaft
    abschalten lässt sie sich unter Einstellungen → KI-Provider →
    „Web-Recherche". Der Hinweis enthält nicht den Fehlertext des
    Providers.
  - Dieselbe Anfrage geht genau einmal ohne Web-Werkzeuge erneut an den
    Provider, mit demselben Mindestabstand und derselben Wartezeit bei
    knappem Rate-Limit-Budget wie jede andere Anfrage. Die Antwort
    erscheint normal unter dem Hinweis.
  - Für den Rest der Sitzung enthält keine Anfrage mehr ein Web-Werkzeug;
    der Hinweis erscheint deshalb je Sitzung höchstens einmal. Die
    gespeicherte Einstellung bleibt unverändert, eine neu verbundene
    Sitzung versucht es wieder mit Web-Recherche.
  - Scheitert auch die erneute Anfrage, zeigt der Chat deren Fehler; es
    folgt kein weiterer Versuch.
  - Jeder andere Fehler (andere ungültige Anfrage, Anmeldung, Rate-Limit,
    Überlastung oder Serverfehler, Budget) erscheint wie bisher und wird
    hierüber nie wiederholt.
- **OpenAI lehnt das Web-Werkzeug ab:** Die oben beschriebene Erkennung
  und der einmalige Wiederholversuch gelten nur für Anthropic. Bei OpenAI
  erscheint ein Fehler des Providers wie jeder andere Provider-Fehler; der
  Zug stürzt nicht ab.
- **Abgeschnittene Antwort:** Wird eine Antwort mit Aktionsvorschlag wegen
  des Längenlimits verworfen und wiederholt (Spec 0065), verfallen auch die
  Recherchen der verworfenen Antwort; angezeigt und gespeichert werden nur
  die der wiederholten. Die Sitzung gilt trotzdem als „nicht
  vertrauenswürdigen Inhalt gelesen", sobald die verworfene Antwort ein
  Web-Ergebnis erhalten hat, weil ihr Text schon gestreamt worden sein kann.

## 8. Netzwerk

Die Web-Recherche ist keine Verbindung dieses Rechners: Die App schickt
nur die ohnehin gesendete Chat-Anfrage an den Provider; Suche und Abruf
führt der Provider aus. Liste der Verbindungen und ihre Absicherung:
`docs/netzwerkverbindungen.md`.
