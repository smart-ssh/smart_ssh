# Spec 0112 — Verbindungsfortschritt und Abbruch

Status: umgesetzt
Zweck: Während eine Verbindung zu einem Server aufgebaut wird, meldet die App jeden Verbindungsschritt live und lässt den laufenden Aufbau abbrechen.
Bezüge: Spec 0005 (SSH, Jump-Hosts), Spec 0069 (Fehlerpfad, Zeitgrenzen), Spec 0076 (Hop-Benennung), Spec 0107 (Wiederverbinden), ADR 0110, ADR 0115.

## 1. Fortschrittsmeldungen

- Jeder Verbindungsaufbau, den die Oberfläche mit einer Versuchskennung (UUID) startet, meldet seinen Fortschritt als Ereignis `connect-progress`. Alle Ereignisse eines Aufbaus tragen diese Kennung und die Server-Kennung.
- Das Ereignis enthält die vollständige aktuelle Schrittliste. Es kommt, sobald ein Schritt beginnt, seine Angabe sich ändert, er gelingt oder scheitert.
- Die Zählung `attempt` beginnt bei 1. Wiederholt die App den Aufbau (z. B. nach „Vertrauen“ bei einem Host-Key), steigt sie um 1 und die Schrittliste beginnt neu.
- Je Schritt: Art (Namensauflösung, TCP-Verbindung, Tunnel je Jump-Hop, Handshake, Host-Key-Prüfung, Anmeldung, Sitzung bereit), Hop-Bezeichnung nach Spec 0076, Status (`running`, `ok`, `skipped`, `failed`), bei `failed` der stabile Fehlercode, und eine kurze Angabe.
- Die Angabe ist ein stabiler Code mit Parametern, kein übersetzter Text:
  - Namensauflösung und TCP: `host:port`, zu dem gerade verbunden wird;
  - Tunnel: die Hop-Bezeichnung;
  - Handshake, solange er läuft: „wartet auf Antwort“;
  - Host-Key-Prüfung, solange die Host-Key-Abfrage offen ist: „wartet auf Ihre Entscheidung“ (der Schritt gilt dann als laufend);
  - Anmeldung: nur die gerade versuchte Methode (`publickey (agent)`, `publickey (key file)`, `password`).
- Ist der Host eine IP-Adresse (IPv4 oder IPv6), findet keine Namensauflösung statt: Der Schritt erscheint mit Status `skipped` und der Bezeichnung „IP-Adresse – keine Auflösung nötig“; bei einem Hostnamen läuft die Auflösung wie bisher.
- Läuft die Zeitgrenze ab, während die TCP-Verbindung aufgebaut wird, trägt der TCP-Schritt den Code `SSH_TCP_TIMEOUT` („TCP-Verbindung hat das Zeitlimit überschritten“); Zeitgrenzen in späteren Schritten behalten `SSH_TIMEOUT`. Das automatische Wiederverbinden (Spec 0107) behandelt beide Codes als wiederholbar.
- Die Oberfläche zeigt bei einem gescheiterten Schritt einen übersetzten Grund statt des rohen Codes; der Code selbst steht nur in der Detailzeile und im kopierten Text.
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

## 4. Verbindungs-Tab (Oberfläche)

- Sobald der Aufbau wirklich beginnt (bei der Sitzungsauswahl nach der Wahl), öffnet sich sofort ein neuer, fokussierter Tab „Verbinde mit <Servername> …“ mit Ladesymbol im Tab. Der Tab wird nach dem Klick geöffnet, nicht erst nach dem Verbindungsergebnis.
- Der Tab-Inhalt listet die Schritte live aus `connect-progress`: eine Zeile je Schritt mit Statussymbol (läuft: Ladesymbol, gelungen: Haken, gescheitert: Kreuz) und der übersetzten Angabe (`host:port`, Hop-Bezeichnung, „wartet auf Antwort“, „wartet auf Ihre Entscheidung“, Anmeldemethode). Die Liste ist eine Liste mit höflicher Live-Ansage (`aria-live="polite"`), die nur abgeschlossene oder gescheiterte Schritte meldet, nicht jede Änderung einer Angabe.
- Bei Erfolg wird derselbe Tab an derselben Stelle und mit demselben Fokus zum normalen Sitzungs-Tab, ohne Zwischenbild und ohne Mindestanzeigedauer.
- Bei einem Fehler bleibt der Tab: Titel „Verbindung zu <Servername> fehlgeschlagen“ mit Fehlersymbol, die Schrittliste mit markiertem gescheitertem Schritt, die übersetzte Fehlermeldung samt Hinweis zum nächsten Schritt (Spec 0024, ADR 0039) und das aufklappbare Schritt-Protokoll (Details), sowie die Knöpfe „Erneut versuchen“ (neuer Versuch im selben Tab mit zurückgesetzter Schrittliste) und „Schließen“. Der Fokus liegt dann auf „Erneut versuchen“.
- Schließen des Tabs oder „Abbrechen“ im laufenden Tab bricht den Aufbau ab (Abschnitt 2) und entfernt den Tab; ein abgebrochener Aufbau zeigt nirgends einen Fehler. Das Schließen eines gescheiterten Tabs entfernt nur den Tab.
- Die Meldung über der Serverliste entfällt für Verbindungsfehler; dort zeigt die Zeile des Servers „Verbindet…“, solange ein Versuch für ihn läuft.
- Ein Klick auf einen Server, für den schon ein Tab (laufender oder gescheiterter Versuch, Sitzung) besteht, wechselt zu diesem Tab, statt einen zweiten Aufbau zu starten (Spec 0017).
- Die Host-Key-Abfrage (Spec 0100) erscheint wie bisher; der Tab zeigt den Host-Key-Schritt währenddessen als „wartet auf Ihre Entscheidung“, nach „Vertrauen“ laufen die Schritte im selben Tab weiter.
- Auch das Fortsetzen einer gespeicherten Chat-Sitzung aus dem Auswahl-Screen (Spec 0034) öffnet sofort den Verbindungs-Tab, mit Schrittliste, Abbrechen und im Fehlerfall Fehler, Schritt-Protokoll sowie „Erneut versuchen" und „Schließen". „Erneut versuchen" setzt dieselbe Chat-Sitzung fort, nicht eine neue. Bei Erfolg wird der Tab zum Sitzungs-Tab mit der gespeicherten Historie; ein Abbruch lässt die gespeicherte Chat-Sitzung unverändert.
- Das automatische Wiederverbinden (Spec 0107) nutzt den Verbindungs-Tab nicht.

## 5. Grenzen

- Keine Mindestanzeigedauer und kein Verlauf der Schrittlisten: sie leben nur im Tab.
