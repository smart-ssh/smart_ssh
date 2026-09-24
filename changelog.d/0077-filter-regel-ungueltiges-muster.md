### Behoben
- Eine Filterregel, deren Muster sich nicht übersetzen lässt, wird beim
  Anlegen und beim Ändern jetzt abgewiesen, statt gespeichert zu werden und
  anschließend wirkungslos in der Liste zu stehen. Das galt bisher für jede
  Aktion — auch für eine Deny- oder Bestätigen-Regel, die dadurch stillschweigend
  nichts tat. Das gilt für das Regel-Formular und für die Schnellregel aus
  dem Bestätigungsdialog.
- Der Bestätigungsdialog schlägt keine Schnellregel mehr vor, deren Muster
  sich nicht übersetzen lässt. Bisher entstand ein solcher Vorschlag aus
  einem Kommando mit einer Klammer im Argument und ließ sich anwählen,
  führte aber zu keiner wirksamen Regel.

### Geändert
- Eine bereits gespeicherte Regel mit einem solchen Muster — etwa aus einer
  älteren Programmfassung — wird in der Regelliste sichtbar markiert, mit
  dem Hinweis, dass das Muster ungültig ist, und der Fundstelle darin.
  Bearbeiten und Löschen bleiben möglich; Speichern verlangt dann ein
  gültiges Muster. Zusätzlich wird eine solche Regel bei jeder Auswertung
  im Protokoll vermerkt (mit Regel-Kennung, nie mit dem Kommando).
- Meldet das Regel-Formular ein ungültiges Muster, steht dort jetzt ein
  verständlicher Satz in der eingestellten Sprache und darunter die genaue
  Fundstelle im Muster — bisher nur der englische Text der zugrunde
  liegenden Bibliothek.
- An einer so markierten Regel lässt sich die Priorität nicht mehr über die
  Pfeiltasten verschieben; die Pfeile sind deaktiviert und nennen den Grund.
