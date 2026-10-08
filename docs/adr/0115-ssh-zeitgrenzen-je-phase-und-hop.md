# ADR 0115 — SSH-Zeitgrenzen je Phase und je Hop

Status: akzeptiert
Betrifft: Spec 0069 (Teil A3, §5), Issue #97; löst den in ADR 0062 §3
festgehaltenen, bewusst nicht behobenen Fund „10 s umschließen auch die
Anmeldung und die ganze Hop-Kette" ab.

## Kontext

Bis Issue #97 lag eine einzige 10-s-Grenze um den ganzen Verbindungsaufbau
(`connect_session` und `test_connection`). Sie umfasste TCP, Handshake und
Anmeldung aller Hops. Ein Schlüssel, der eine Berührung oder eine
Agent-Bestätigung braucht, und eine längere Jump-Host-Kette brachen dadurch
mit `SSH_TIMEOUT` ab, obwohl nichts hing. Entschieden ist: getrennte Grenzen
je Phase, je Hop (Werte in Spec 0069). Offen ließ das Issue, ob außen noch
eine Gesamtgrenze bleibt, und wie die Grenzen testbar werden.

## Entscheidung

1. **Grenzen im Transport, nicht beim Aufrufer.** `ssh_transport::
   ConnectLimits { handshake, authentication }` mit `ConnectLimits::DEFAULT`
   (10 s / 60 s). `connect_with_limits` setzt sie je Phase; `connect` und
   `connect_with_log` laufen mit `DEFAULT`, damit kein künftiger Aufrufer
   versehentlich ohne Grenze verbindet. `app_logic::test_connection::
   SSH_CONNECT_LIMITS` ist die eine Konstante, die `connect_session` und
   „Verbindung testen" nutzen.
2. **Phasen hinter einem Trait.** `drive_chain` verkettet die Hops und legt
   um jede Phase ihre Grenze (über `connect_with_timeout`); die Phasen selbst
   liefert ein privater Trait `HopConnector` (`open_first`, `open_next`,
   `authenticate`), real umgesetzt von `RusshHops`. So lassen sich die
   Grenzen mit pausierter Tokio-Uhr ohne Netzwerk prüfen — ein echter
   Zwei-Hop-Test ist wegen ADR 0008 nicht verfügbar, und eine verzögerte
   Anmeldung lässt sich gegen den Testserver nicht erzeugen. Der Trait sieht
   keinen `HostKeyStore`, den `drive_chain` beschreiben könnte: ein Timeout
   kann strukturell kein `trust()` auslösen.
3. **Phase „Verbindung und Handshake"** umfasst beim ersten Hop
   Namensauflösung, TCP, Handshake mit Host-Key-Prüfung und die nachträgliche
   DNS-Diagnose im Fehlerfall (Spec 0069 §4: `lookup_host` läuft innerhalb
   einer Grenze); bei weiteren Hops den Tunnel über den vorherigen Hop und
   den Handshake.
4. **Äußeres Sicherheitsnetz: ja, aus der Hop-Zahl berechnet** (Option b
   des Issues). `ConnectLimits::overall(n)` = n × (Handshake + Anmeldung) +
   eine weitere Handshake-Grenze. Weil die Phasen nacheinander laufen und
   jede begrenzt ist, greift das Netz im normalen Ablauf nie vor einer
   Phasengrenze; es fängt nur einen Abschnitt außerhalb einer Phase ab.
   `connect_session` und `test_connection` umschließen den Versuch damit.

## Konsequenzen

- Ein Ablauf in einer Phase schließt den laufenden Schritt im
  Schritt-Protokoll selbst (`connect_with_limits` → `fail_running`); greift
  das äußere Netz, schließt ihn wie bisher der Aufrufer.
- Der `Connector`-Trait in `app-logic` bekommt die Grenzen als Parameter,
  damit „Verbindung testen" nachweislich dieselben nutzt.
- Ein hängender Server scheitert weiter nach 10 s; eine Kette mit n Hops
  kann im Extremfall bis n × 70 s brauchen, bevor eine Phase abläuft — der
  Nutzer sieht im Schritt-Protokoll, welcher Schritt läuft.

## Verworfen

- Eine Grenze über alles beibehalten: Fehlabbrüche bei Touch-Schlüsseln.
- Nur TCP und Handshake begrenzen: die Anmeldung könnte wieder ohne Ende
  hängen.
- Kein äußeres Netz: jede künftig eingefügte, nicht begrenzte Stelle im
  Aufbau könnte wieder unbegrenzt warten.
