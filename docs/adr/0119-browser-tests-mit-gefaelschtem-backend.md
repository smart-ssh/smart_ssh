# ADR 0119 — Browser-Tests mit gefälschtem Tauri-Backend

Status: akzeptiert
Betrifft: Issue #166

## Kontext

Die Vitest-Tests des Frontends laufen in jsdom. jsdom hat keine
Layout-Engine: Ob ein Knopf im Fenster liegt, verdeckt ist oder ob eine
CSS-Klasse im gebauten Stylesheet überhaupt existiert, sieht es nicht.
Issue #166 verlangt Tests in echten Browser-Engines (Chromium und WebKit,
zwei Fenstergrößen) gegen ein gefälschtes Backend über
`@tauri-apps/api/mocks`. Offen ließ das Issue, wie der Testeinstieg
ausgeliefert wird, wie das gefälschte Backend aufgebaut ist und wie einige
Fälle konkret zu lesen sind.

## Entscheidung

1. **Testeinstieg über den Vite-Dev-Server.** `e2e/harness/index.html`
   liegt neben dem echten `index.html` und wird vom Dev-Server
   ausgeliefert, ist aber kein Eingang von `vite build`. Gefälschtes
   Backend, Playwright und axe landen dadurch nie im ausgelieferten
   Bundle. Der Dev-Server statt `vite preview` testet dieselben Quellen
   ohne vorherigen Build. Eine eigene Vite-Konfiguration (`e2e/
   vite.config.ts`) übernimmt die der App und nimmt den Testeinstieg in die
   Abhängigkeits-Vorabanalyse auf; sonst entdeckt Vite
   `@tauri-apps/api/mocks` erst zur Laufzeit und lädt die Seite mitten im
   Test neu.
2. **Gefälschtes Backend als kleines Modell.** Server, Gruppen, Sitzungen,
   Einstellungen und Host-Keys liegen in einem Objekt; die Kommandos, die
   die getesteten Bildschirme senden, arbeiten darauf (z. B.
   `move_server_to_group` ändert die Gruppe, `connect` fragt bei
   unbekanntem Host-Key nach und wartet auf `confirm_host_key`). Ein Test
   kann jedes Kommando mit einer festen Antwort, einem Fehler oder einem
   zurückgehaltenen Aufruf überschreiben und Backend-Ereignisse
   auslösen. Ereignisse verwaltet das gefälschte Backend selbst (statt
   `shouldMockEvents`), damit ein Test warten kann, bis ein Listener
   angemeldet ist.
3. **Unbekannte Kommandos scheitern laut.** Ein unbekanntes Kommando wird
   mit einer Fehlermeldung abgelehnt, die den Namen nennt, und
   protokolliert. Weil die App manche Fehler nur loggt, prüft die
   Test-Fixture nach jedem Test zusätzlich, dass kein unbekanntes Kommando
   aufgerufen wurde.
4. **Neu laden behält den Zustand.** Das Modell liegt in
   `sessionStorage`. Ein Reload derselben Seite (Fall 2: der
   Erststart-Hinweis bleibt bestätigt) sieht deshalb den gespeicherten
   Zustand, ein neuer Test beginnt mit leerem Speicher.
5. **Layout ohne Bildvergleich.** Statt Screenshots prüfen Hilfsfunktionen
   die Geometrie: Box ganz im Fenster, oberstes Element in der Mitte
   (`elementFromPoint`), per Tab erreichbar, nur der Inhaltsbereich
   scrollt. Das ergibt auf jedem Betriebssystem dasselbe Ergebnis. WebKit
   unter macOS überspringt Knöpfe bei Tab (Safari-Vorgabe); dort nutzt die
   Hilfsfunktion Option+Tab, die Tastenkombination, die auf dieser
   Plattform jedes Bedienelement erreicht.
6. **Auslegung einzelner Fälle.**
   - Das Verbindungs-Schrittprotokoll ist kein eigener Dialog, sondern
     steht unter der Fehlermeldung in der Serverliste bzw. im
     Serverformular. Fall 5 prüft es dort: Ein langes Protokoll scrollt mit
     dem Inhaltsbereich, die Navigation der App bleibt sichtbar und
     bedienbar.
   - Die Notiz-Diff-Vorschau prüft Fall 5 an der aufgeklappten
     Notiz-Vorschlags-Karte (sie ist das Dialog-Äquivalent für Vorschläge,
     die nach dem Trennen eintreffen).
   - Fall 9 erkennt einen rohen Schlüssel an der Form `abschnitt.schlüssel`,
     wobei `abschnitt` ein Abschnitt des Übersetzungskatalogs ist. Ein
     Schlüssel, der nur im deutschen Katalog fehlt, zeigt den englischen
     Text (Rückfallsprache) und ist deshalb kein roher Schlüssel.
   - Fall 10 prüft die Einstellungen in jeder Kategorie, nicht nur in der
     ersten.
7. **Bekannte axe-Befunde stehen in einer Ausnahmeliste** (`e2e/support/
   axe.ts`) mit Begründung. Heute: zu geringer Farbkontrast im dunklen
   Thema (app-weite Designfrage) und der nicht fokussierbare,
   scrollbare Konfigurationsausschnitt in den MCP-Einstellungen. Das
   unbeschriftete Kommandofeld im Bestätigungsdialog wurde stattdessen
   behoben.

## Abgrenzung

- **Fall 4 prüft keinen Schlüsseltyp.** Das Issue nennt für den
  Host-Key-Dialog „Host, Schlüsseltyp und Fingerprint“. Auf `main` zeigt
  der Dialog `host:port` und den SHA-256-Fingerprint, aber keinen
  Schlüsseltyp, und keine Spec verlangt ihn (Spec 0005 und Spec 0007
  nennen nur den Fingerprint; das Ereignis
  `host-key-verification-needed` trägt kein Feld dafür). Fall 4 prüft
  deshalb Host, Port und Fingerprint (unbekannter und geänderter
  Schlüssel), dass ohne Klick nichts akzeptiert wird, dass „Ablehnen“ die
  Verbindung nicht herstellt und dass beide Knöpfe in beiden Fenstergrößen
  bedienbar bleiben. Den Schlüsseltyp anzuzeigen wäre eine eigene
  Produktänderung (Ereignis, Dialog, Spec 0007) und gehört in ein eigenes
  Issue; Fall 4 kann dann erweitert werden.

## Konsequenzen

- Neue Bildschirme oder Kommandos brauchen einen Handler im gefälschten
  Backend oder eine Überschreibung im Test; vergisst man das, scheitert
  der Test mit dem Namen des Kommandos.
- Die Tests prüfen das Frontend gegen ein angenommenes Backend-Verhalten.
  Ob das echte Backend so antwortet, prüfen weiterhin die Rust-Tests.
- Die CI braucht die Browser von Playwright samt Systemabhängigkeiten
  (`npx playwright install --with-deps chromium webkit`).
