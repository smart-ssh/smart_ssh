### Behoben
- „Zugangsdaten testen“ für einen Anthropic-Provider schlug fälschlich mit
  „Provider nicht erreichbar“ fehl, obwohl der Schlüssel gültig war — die
  Probe schickte keinen System-Prompt, und Anthropic lehnt einen leeren
  System-Textblock mit Caching-Markierung ab.
