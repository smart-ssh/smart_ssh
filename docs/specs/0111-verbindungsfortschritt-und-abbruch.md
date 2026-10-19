# Spec 0111 — Verbindungsfortschritt und Abbruch

Status: umgesetzt
Zweck: Während eine Verbindung zu einem Server aufgebaut wird, meldet die App jeden Verbindungsschritt live und lässt den laufenden Aufbau abbrechen.
Bezüge: Spec 0005 (SSH, Jump-Hosts), Spec 0069 (Fehlerpfad, Zeitgrenzen), Spec 0076 (Hop-Benennung), Spec 0107 (Wiederverbinden), ADR 0110, ADR 0115.

## 1. Fortschrittsmeldungen

- Jeder Verbindungsaufbau, den die Oberfläche mit einer Versuchskennung (UUID) startet, meldet seinen Fortschritt als Ereignis `connect-progress`. Alle Ereignisse eines Aufbaus tragen diese Kennung und die Server-Kennung.
- Das Ereignis enthält die vollständige aktuelle Schrittliste. Es kommt, sobald ein Schritt beginnt, seine Angabe sich ändert, er gelingt oder scheitert.
- Die Zählung `attempt` beginnt bei 1. Wiederholt die App den Aufbau (z. B. nach „Vertrauen“ bei einem Host-Key), steigt sie um 1 und die Schrittliste beginnt neu.
- Je Schritt: Art (Namensauflösung, TCP-Verbindung, Tunnel je Jump-Hop, Handshake, Host-Key-Prüfung, Anmeldung, Sitzung bereit), Hop-Bezeichnung nach Spec 0076, Status (`running`, `ok`, `failed`), bei `failed` der stabile Fehlercode, und eine kurze Angabe.
- Die Angabe ist ein stabiler Code mit Parametern, kein übersetzter Text:
  - Namensauflösung und TCP: `host:port`, zu dem gerade verbunden wird;
  - Tunnel: die Hop-Bezeichnung;
  - Handshake, solange er läuft: „wartet auf Antwort“;
  - Host-Key-Prüfung, solange die Host-Key-Abfrage offen ist: „wartet auf Ihre Entscheidung“ (der Schritt gilt dann als laufend);
  - Anmeldung: nur die gerade versuchte Methode (`publickey (agent)`, `publickey (key file)`, `password`).
- Die Meldungen enthalten nie Passwort, Passphrase, Schlüsselmaterial oder den Pfad einer Schlüsseldatei.
- Scheitert der Aufbau, endet die Meldungsfolge mit dem gescheiterten Schritt (z. B. Anmeldung mit Fehlercode, Handshake mit dem Zeitüberschreitungs-Code). Der Verbindungsfehler trägt das Schritt-Protokoll unverändert wie bisher.
- Das automatische Wiederverbinden (Spec 0107) sendet keine `connect-progress`-Ereignisse.

## 2. Abbrechen

- Der Befehl `cancel_connect` mit der Versuchskennung bricht den laufenden Aufbau ab, auch während das Fenster der Host-Key-Abfrage offen ist.
- Der Aufbau endet dann mit dem Fehlercode `SSH_CONNECT_CANCELLED`, ohne Wiederholung, ohne Sitzung und ohne Chat-Sitzungseintrag. Offene Verbindungen und Tunnel des Versuchs werden geschlossen.
- Wartet der Aufbau auf eine Host-Key-Entscheidung, gilt der Schlüssel als nicht vertraut; die App meldet das Ende der Abfrage wie bei jedem anderen Ausgang, damit das Fenster schließt.
- Der Abbruch wirkt innerhalb einer Sekunde.
- Ein Abbruch für eine unbekannte oder bereits beendete Versuchskennung tut nichts und meldet keinen Fehler.

## 3. Sicherheitszusagen

- Die Meldungen sind reine Anzeige; keine Vertrauens- oder Anmeldeentscheidung hängt an ihnen.
- Ein Abbruch vertraut nie einen Host-Key.

## 4. Grenzen

- Die Oberfläche zum Anzeigen und Abbrechen ist nicht Teil dieser Spec.
