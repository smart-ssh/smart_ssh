# ADR 0123 — SFTP: Abbruch der Sitzung über den Zustand der SSH-Sitzung erkennen

Status: akzeptiert
Betrifft: Issue #155, Spec 0069 (Teil A3, „Abbruch einer laufenden Sitzung"), Spec 0020 (§4.3), Spec 0067

## Problem

Spec 0069 verlangt `SSH_SESSION_CLOSED`, wenn die Verbindung während einer
SFTP-Operation abbricht. Jede SFTP-Operation und der SFTP-Handshake meldeten
aber jeden Fehler außer `PermissionDenied` als `SSH_CHANNEL_ERROR`.

Der SFTP-Client (`russh-sftp` 2.4) unterscheidet nicht, **warum** sein Kanal
endete. Ob die ganze SSH-Sitzung weg ist oder nur dieser eine Kanal (der
`sftp-server` beendet sich, sudo lehnt den erhöhten Start über einen
Exec-Kanal ab, Spec 0067), sieht der Aufrufer gleich:

- eine Anfrage, die beim Ende des Kanals noch unterwegs ist, endet mit
  `Timeout`: die Tabelle offener Anfragen teilt sich der beendete Lese-Task
  mit der Sitzung, die Antwort-Sender bleiben bestehen;
- jede weitere Anfrage endet mit `UnexpectedBehavior("session closed")`;
- daneben gibt es `IO(_)` (nur noch der Text des `io::Error`), „sender
  dropped" und die aus `SendError`/`RecvError` erzeugten Meldungen.

Eine Regel, die allein auf diese Fehler schaut, hätte einen von sudo
abgelehnten Start als „Verbindung unterbrochen, neu verbinden" gemeldet,
obwohl die Sitzung steht.

## Entscheidung

1. **Zwei Signale.** `SessionClosed` gibt es, wenn
   - der Server-Status `NoConnection` oder `ConnectionLost` lautet, oder
   - der Fehler einen geschlossenen Kanal anzeigt (`IO`, `Timeout`, die
     genannten `UnexpectedBehavior`-Meldungen; beim Schreiben über einen
     Datei-Handle dieselben Texte als `io::Error` bzw. `BrokenPipe`) **und**
     der `russh`-Handle der Sitzung `is_closed()` meldet.
2. **Alles andere bleibt, wie es war.** `PermissionDenied` bleibt
   `SftpPermissionDenied` (Sudo-Fallback, Spec 0020 §4.3), übrige
   Statuscodes, `Limited`, `UnexpectedPacket` und sonstiges
   Protokollverhalten bleiben `ChannelError` mit Pfad, auch bei beendeter
   Sitzung. Ein `Timeout` bei lebender Sitzung ist eine echte
   Zeitüberschreitung und bleibt `ChannelError`. Die äußere Frist des
   Exec-Starts ist unverändert.
3. **Nur ein `Weak`-Verweis.** Der Transport hält den Handle jetzt in einem
   `Arc`; jede SFTP-Sitzung bekommt nur einen `Weak`-Verweis darauf. Ein
   offener SFTP-Kanal hält die SSH-Sitzung damit nicht über das Ende ihres
   Transports hinaus am Leben. Ist der Transport schon weg, gilt die Sitzung
   als beendet.
4. **Reihenfolge in `russh`.** Beim Sitzungsende schließt `russh` zuerst die
   Nachrichten-Warteschlange des Handles (`is_closed()` wird `true`) und gibt
   erst danach die Kanäle frei. Endet der SFTP-Kanal wegen des
   Sitzungsendes, ist das Signal beim Fehler also schon gesetzt. Endet ein
   Kanal vorher aus anderem Grund, bleibt es `ChannelError`, wie bisher.

## Verworfene Alternativen

- **Nur die Fehlerart** (Vorschlag im Issue): meldet einen abgelehnten
  sudo-Start und einen beendeten `sftp-server` als Verbindungsabbruch, und
  erfasst den häufigen Fall einer laufenden Anfrage (`Timeout`) nicht.
- **Den Handle selbst teilen (`Arc`)**: hielte die Sitzung am Leben, solange
  irgendein SFTP-Kanal existiert — eine Änderung der Lebensdauer, nicht nur
  der Einordnung.

## Konsequenzen

- Die Erkennung hängt an den Fehlertexten von `russh-sftp` 2.4. Tests prüfen
  sie gegen echte Fehler der Bibliothek (Stream-Ende nach dem Handshake);
  ändert ein Upgrade die Texte, schlagen sie fehl. Bis dahin fällt eine
  nicht erkannte Meldung auf den bisherigen `ChannelError` zurück, nie auf
  Erfolg.
- Eine laufende Anfrage meldet den Abbruch erst nach ihrer Frist (10 s, beim
  Exec-Handshake 5 s); das ist Verhalten der Bibliothek und unverändert.
