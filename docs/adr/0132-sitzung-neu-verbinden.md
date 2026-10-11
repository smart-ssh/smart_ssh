# ADR 0132 — Neu verbinden: Transport in der Sitzung austauschen

Status: akzeptiert
Betrifft: Issue #355, Spec 0107, Spec 0085/0086 (gekapselter Transport)

## Kontext

Die Sitzung kapselt ihren Transport und den normalen Dateikanal absichtlich
so, dass sich beide von außen nicht ersetzen lassen (Spec 0085/0086): Der
offene Kanal gehört zum Transport, ein Austausch nur des einen bräche die
Paarung. Neu verbinden muss aber genau diese Bestandteile austauschen.

## Entscheidung

1. Der Austausch läuft über eine einzige Stelle in `app-logic`:
   `Session::begin_reconnect()` liefert einen `ReconnectClaim`, nur dessen
   `succeed(transport, redactor)` tauscht Transport, Dateikanal und
   Redactor gemeinsam. Der Claim setzt den Zustand atomar von
   `Disconnected` auf `Reconnecting` (zweiter Auslöser bekommt `None`) und
   fällt ohne `succeed` auf `Disconnected` zurück. Die `compile_fail`-Tests
   aus Spec 0086 bleiben unverändert gültig.
2. Der Verbindungsaufbau (Jump-Hosts, Zugangsdaten, Host-Key-Schleife) ist
   aus `connect_session` in `establish_transport` herausgezogen und wird von
   beiden Wegen genutzt. Dadurch gibt es keine zweite Prüfstrecke und keine
   gemerkte Zustimmung.
3. Der Redactor ist ein austauschbarer Behälter (`SwappableRedactor`), damit
   ein neu gelesenes Sudo-Passwort sofort redigiert wird. `Session.
   sudo_password` (nur für `sudo -S`-Eingaben) bleibt der Wert vom ersten
   Verbinden: ein geändertes Passwort lässt `sudo` mit dem alten
   scheitern, es wird dabei nichts offengelegt.
4. Das Startverzeichnis wird nicht neu geprüft; das einmalige Ergebnis der
   Sitzung (Spec 0102) gilt weiter, damit der Hinweis „Verzeichnis fehlt“
   nicht erneut erscheint.
5. Eine wartende Bestätigung wird wie beim Schließen des Tabs abgelehnt
   (fail closed). Sie bleibt im Verlauf eine Ablehnung, keine Ausführung.
