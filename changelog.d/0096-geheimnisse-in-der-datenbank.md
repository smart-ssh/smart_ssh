### Geändert

- Der automatisch erzeugte Titel einer Chat-Sitzung wird geschwärzt, bevor er
  gespeichert wird. Der Titel steht als Einziges der Sitzung im Klartext in
  der Datenbank — hatte die KI ein Passwort aus dem Gespräch in den Titel
  übernommen, stand es dort bisher lesbar. Besteht der Titel danach nur noch
  aus Platzhaltern, wird gar kein Titel gespeichert.
- Schlägt die KI eine Notiz vor, wird ihr Vorschlag geschwärzt, **bevor** er
  im Vergleichsdialog erscheint — im Chat, beim Trennen der Verbindung und
  bei einem Vorschlag über MCP. Angezeigt, bestätigt und gespeichert wird
  dieselbe Fassung. Bestand der Vorschlag ausschließlich aus erkannten
  Zugangsdaten, entsteht kein Vorschlag, und die App sagt das.
- Unverändert bleibt „In Notiz übernehmen": Was Sie selbst aus dem Chat in
  eine Notiz übernehmen, wird nicht angetastet.
