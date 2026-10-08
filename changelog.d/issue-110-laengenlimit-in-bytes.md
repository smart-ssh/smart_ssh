### Sicherheit
- Das Längenlimit für Kommandos (4096) wird jetzt überall in Bytes gemessen.
  Bisher zählte die Filter-Engine Zeichen, die Risiko-Einschätzung Bytes; ein
  Kommando mit vielen Mehrbyte-Zeichen (z. B. „€") konnte deshalb über eine
  Allow-Regel automatisch laufen, obwohl die Risiko-Einschätzung es nicht
  mehr geprüft hatte. Solche Kommandos verlangen jetzt immer eine
  Bestätigung.
