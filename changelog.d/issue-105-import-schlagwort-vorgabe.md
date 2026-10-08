### Sicherheit
- `ssh_config`-Import: Jedes importierte Schlagwort, das eine bestehende
  Tag-Allow-Regel trifft, ist in der Vorschau jetzt standardmäßig
  abgewählt — auch Muster-Schlagworte wie `*.prod.de`, nicht mehr nur
  buchstäbliche. Trifft das Schlagwort zugleich eine Tag-Deny-Regel,
  bleibt es angewählt, damit die Deny-Regel weiter greift.
