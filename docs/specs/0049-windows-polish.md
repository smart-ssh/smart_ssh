# Spec 0049 — Windows-Politur: Zugangsdaten-Trim, Anbieter-Fehler im Log, Fenster-Steuerung

Status: umgesetzt
Zweck: Vier Funde aus dem ersten Windows-Test: eingefügte Zugangsdaten mit Zeilenumbruch am Ende, ein Anbieter-Fehler ohne erkennbaren Grund im Log, fehlende Fenster-Buttons und ein nicht ziehbares Fenster.
Bezüge: Spec 0007 und 0008 (Zugangsdaten), Spec 0014 (Titelleiste), Spec 0016 (Logging, Redaction), Spec 0047 (verständliche Fehlermeldungen), Spec 0073 (geteilter Trim, erweitert Fund 1), ADR 0040 (Titelleiste: Plugin-Aktivierung statt eigener Buttons), ADR 0064.
Review-Priorität: ERHÖHT für Fund 1 (Credential-Handling) und Fund 2 (Redaction), sonst NORMAL.

Die Abschnitte tragen die Kennungen der Funde (1, 2, 3/4), auf die Code und
Tests verweisen.

## Fund 1 — Rand-Whitespace in eingefügten Zugangsdaten wird entfernt

Beim Einfügen unter Windows hängt oft ein `\r\n` an einem API-Key; ungetrimmt
scheitert die Anmeldung beim Anbieter („Credentials ungültig"). Deshalb
werden Zugangsdaten beim Speichern und beim Verbindungstest an beiden
Rändern von Leerraum (Leerzeichen, Tabulator, `\r`, `\n`) befreit:

- API-Key eines KI-Anbieters, dazu seine Endpunkt-Felder (Base-URL und
  Attestierungs-URL),
- Passwort, Sudo-Passwort und Key-Passphrase eines Servers.

Regeln:

- Die Bereinigung gilt unabhängig vom Eingabeweg (Einfügen, Tippen, Import)
  und steht an einer zentralen Stelle der Speicherlogik, nicht in der
  Oberfläche.
- Nur die Ränder werden angefasst; Zeichen im Inneren eines Wertes bleiben
  unverändert.
- Erst wird getrimmt, dann entschieden, ob die Eingabe leer ist; ein nach dem
  Trimmen leerer Wert bedeutet weiterhin „unverändert lassen" (Spec 0007).
- Ein Passwort, das absichtlich mit einem Leerzeichen endet, wird bewusst
  nicht unterstützt: Whitespace am Rand ist bei Zugangsdaten selten, ein
  angehängter Zeilenumbruch beim Einfügen häufig.
- Spec 0073 erweitert diese Bereinigung um unsichtbare Kopier-Zeichen (BOM,
  Zero-Width-Space u. ä.).

## Fund 2 — Fehlerantwort des Anbieters wird (redigiert) geloggt

Neben der ausgehenden Anfrage wird bei jedem fehlgeschlagenen Anbieter-
Aufruf (Anmeldung, Rate-Limit, Netzwerk, Modell nicht gefunden) auch die
Fehlerantwort geloggt: Status und die Fehlermeldung des Anbieters, z. B. 401
„invalid x-api-key". Damit zeigt das Log bei einem Tester-Problem den echten
Grund; die verständliche Meldung im UI (Spec 0047, Fund D2) bleibt daneben
bestehen.

Das gilt für Chat, Verbindungstest und Modell-Erkennung gleichermaßen. Der
API-Key und weitere Secrets (bei OpenAI-kompatiblen Anbietern auch die Werte
zusätzlicher Header) erscheinen nie im Log: kommt ein solcher Wert wörtlich in
der Antwort vor, wird er vor dem Schreiben ersetzt.

## Fund 3/4 — Fenster-Steuerung und Ziehen auf Windows (und Linux)

Die eigene Titelleiste (Spec 0014) zeigt auf Windows und Linux die
Fenster-Steuerung Minimieren / Maximieren (Wiederherstellen) / Schließen
rechts und lässt das Fenster an der Leiste ziehen. Auf macOS bleiben die
Ampel-Buttons links unverändert.

- Die Leiste reserviert Platz für die Steuerung auf der jeweiligen Seite; die
  Controls und der Versions-Text (Spec 0052) stören sich nicht.
- Ziehen funktioniert auf der freien Fläche der Leiste, nicht auf den
  Buttons; ein Klick auf einen Button löst kein Ziehen aus. Bei geöffnetem
  Dialog bleibt die Leiste ziehbar (Spec 0014).
- Schlägt die Aktivierung der eigenen Titelleiste fehl, stellt die Anwendung
  die native Titelleiste des Betriebssystems wieder her und loggt den
  Fehler. Das Fenster bleibt dann bedienbar, und die Leiste reserviert keinen
  Platz für Controls, die es nicht gibt.

## Sicherheitszusagen

- Nur Rand-Whitespace wird getrimmt; der Inhalt eines Secrets wird nie
  verändert (Fund 1).
- Kein Secret gelangt in das Anbieter-Fehler-Log (Fund 2).

## Grenzen

- Ein Passwort mit absichtlichem Rand-Whitespace ist nicht möglich (Fund 1).
- Das Verhalten der nativen Fenster-Steuerung selbst (Snap-Layouts u. ä.)
  liefert das Betriebssystem bzw. das Titelleisten-Plugin.
