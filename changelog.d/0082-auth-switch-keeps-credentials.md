### Behoben
- Beim Wechsel der Anmeldeart gingen die bisherigen Zugangsdaten verloren,
  wenn das Speichern scheiterte. Wer einen Server mit Passwort auf
  „Zertifikat" umstellte und die Felder leer ließ, bekam zwar richtig eine
  Fehlermeldung — das Passwort war danach aber aus dem Schlüsselbund
  entfernt, während der Server weiter als Passwort-Anmeldung gespeichert
  war. Er ließ sich nicht mehr verbinden, und nichts deutete darauf hin,
  warum. Dasselbe galt, wenn der Schlüsselbund, das Sudo-Passwort oder das
  Speichern selbst scheiterte. Zugangsdaten der bisherigen Anmeldeart
  werden jetzt erst entfernt, nachdem die neue erfolgreich gespeichert ist.
