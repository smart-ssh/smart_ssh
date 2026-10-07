### Neu
- Unter dem Ergebnis von „Verbindung testen" und unter einem
  Verbindungsfehler in der Serverliste lässt sich jetzt ein
  Schritt-für-Schritt-Protokoll aufklappen: DNS-Auflösung, TCP-Verbindung,
  SSH-Handshake (Serverversion, Algorithmen), Host-Key-Prüfung
  (Fingerprint, bekannt/unbekannt/geändert) und Anmeldung mit Dauer und
  Ergebnis, bei Jump-Hosts je Hop. Der gescheiterte Schritt ist markiert,
  „Kopieren" legt das Protokoll in die Zwischenablage. Lehnt der Server die
  Anmeldung ab, stehen die Methoden dabei, die er noch anbietet — sie werden
  nur angezeigt, nicht ausprobiert. Das Protokoll wird nicht gespeichert und
  landet weder in der Log-Datei noch im Diagnose-Export.
