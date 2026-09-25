### Neu
- Server-Profile lassen sich nach OpenSSH-`ssh_config` exportieren — für den
  Umstieg auf ein anderes Werkzeug oder einfach als lesbares Backup. Die
  erzeugte Datei wird von `ssh` selbst akzeptiert (geprüft mit `ssh -G`).
  Nach dem Export zeigt eine Meldung den Pfad, umbenannte Servernamen (falls
  ein Name keinen gültigen `Host`-Alias ergab) und die Zeile, mit der sich
  die Datei in die eigene `~/.ssh/config` einbinden lässt.
- Was sich nicht abbilden lässt — Gruppen, Schlagworte, Notizen,
  Filterregeln, Sicherheitseinstellungen und die Art der Anmeldung —
  verschwindet nicht stillschweigend: Für jeden betroffenen Server steht ein
  Kommentar direkt über seinem Eintrag in der Datei.

### Sicherheit
- Kein Export schreibt je ein Passwort, eine Passphrase oder Schlüsselinhalt
  — nur, bei entsprechender Anmeldeart, den Pfad zu einer Schlüsseldatei.
  Ein Server, dessen Schlüssel im Schlüsselbund liegt, verliert dabei keine
  Information stillschweigend: Der Kommentar über seinem Eintrag sagt, dass
  der Schlüssel fehlt und warum.
- Der Export lehnt die eigene `~/.ssh/config` als Ziel ab — auch wenn sie im
  Speichern-Dialog ausdrücklich gewählt und ein Überschreiben bestätigt
  wird. Diese Datei wird nie angerührt.
- Der lokale Pseudo-Server erscheint nie in der exportierten Datei.
