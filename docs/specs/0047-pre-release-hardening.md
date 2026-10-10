# Spec 0047 — Härtung für die 0.x-Testphase

Status: umgesetzt
Zweck: Fünf Zusagen, die verhindern, dass ein Tester Daten verliert oder mit einem unerklärlichen Fehler allein bleibt: kein verwaister Zugangsdaten-Eintrag nach einem fehlgeschlagenen Anlegen, Daten überleben ein Update, der Start hinterlässt immer eine Logspur, ein nicht erreichbarer Schlüsselbund bricht den Start nicht spurlos ab, die häufigsten Fehler sind verständlich.
Bezüge: Spec 0004 (SQLite-Persistenz), Spec 0008 (Server-Verwaltung), Spec 0016 (strukturiertes Logging), Spec 0024 (Fehlercodes, i18n), Spec 0059 (Startfehler-Dialog), Spec 0069 (Fehlermeldungen des Fünf-Minuten-Pfads), Spec 0101 (Schlüsselbund und Secrets in der Datenbank).
Review-Priorität: ERHÖHT für Fund A2 (Credential-Handling), sonst NORMAL.

Die Abschnitte tragen die Kennungen der Funde (A2, A3, B1, B2, D2), auf die
Code und Tests verweisen.

## Fund A2 — Kein verwaister Zugangsdaten-Eintrag nach fehlgeschlagenem Anlegen

Beim Anlegen eines Servers werden Zugangsdaten (Passwort, Private Key,
Passphrase, Zertifikat, Sudo-Passwort) gespeichert, bevor der Server selbst
gespeichert wird. Schlägt irgendein Schritt danach fehl — das Auflösen der
Anmeldeart, das Sudo-Passwort oder das Speichern des Servers —, entfernt die
Anwendung **alle** Zugangsdaten-Einträge, die dieser Aufruf angelegt haben
könnte, bevor sie den Fehler meldet. Für eine Server-Kennung, die es nicht
gibt, bleibt nichts zurück.

- Ein Eintrag, der ohnehin nie angelegt wurde, ist beim Aufräumen kein
  Fehler.
- Schlägt das Aufräumen selbst fehl (Speicher nicht erreichbar), wird das
  geloggt („Keychain-Rollback fehlgeschlagen: möglicherweise verwaister
  Eintrag"); der ursprüngliche Fehler bleibt der, den der Nutzer sieht.
- Eine ungültige Eingabe (z. B. ein ungültiger SFTP- oder Startverzeichnis-
  Pfad, ein lokaler Pseudo-Server als Jump-Host) wird abgelehnt, **bevor**
  irgendein Zugangsdaten-Eintrag geschrieben wird.

## Fund A3 — Daten überleben den Versionssprung

Die Tester behalten ihre Daten über mehrere Builds: Server, Regeln,
Sitzungen, Notizen und die Verweise auf Zugangsdaten bleiben nach einem
Update vollständig erhalten und lesbar. Die Datenbank wird beim Start
vorwärts migriert; eine Datenbank mit älterem Schema und vorhandenen Daten
verliert dabei keine Zeile. Ist die Datenbank neuer als das Programm, wird
sie nicht angefasst (Spec 0059, Fall 1).

## Fund B1 — Der Start hinterlässt immer eine Logspur

Die Logdatei wird als eine der allerersten Anweisungen des Starts angelegt,
vor dem Öffnen der Datenbank, der Migration und jedem Schlüsselbund-Zugriff.
Auch ein Panic wird mit seiner Meldung ins Log geschrieben, bevor das
Programm endet.

- Die erste Logzeile („Smart SSH startet") enthält App-Version mit Commit-
  Hash (Spec 0052), Build-Typ, Betriebssystem, Architektur und den
  aufgelösten Datenpfad.
- Die wesentlichen Startschritte schreiben je eine Zeile: Prüfung der
  Schlüsselbund-Verfügbarkeit, Beginn und Ende der Datenbank-Migration, und
  als letzter Schritt „app setup complete, creating main window". Aus dem
  Log lässt sich ablesen, wie weit der Start kam.
- Ein fataler Startfehler wird *zusätzlich* zum Log in einem Dialog gezeigt
  (Spec 0059), nie stattdessen.

## Fund B2 — Nicht erreichbarer Schlüsselbund führt nicht zum Absturz

Ist der Schlüsselbund beim Start gesperrt, verweigert oder nicht vorhanden,
stürzt die Anwendung nicht ab und verschwindet nicht spurlos. Der Zustand
wird geloggt; der Nutzer bekommt je nach Lage die Wahl „Erneut versuchen" /
„Beenden" oder das Angebot eines Master-Passworts (Spec 0101, Spec 0059).
Es wird nie still ein neuer Schlüssel erzeugt und nichts an den Daten
verändert.

## Fund D2 — Die fünf häufigsten Fehler sind verständlich

Für die fünf Stolpersteine des Fünf-Minuten-Pfads sieht der Nutzer im UI
(nicht nur im Log) eine Meldung in der eingestellten Sprache, die die
Ursache nennt und den nächsten Schritt:

1. falscher API-Key,
2. Modell nicht gefunden,
3. lokaler Anbieter (Ollama) nicht gestartet,
4. Host nicht erreichbar,
5. Host-Key unbekannt oder geändert (die beiden Host-Key-Dialoge).

Ein Verbindungsfehler trägt dazu seinen stabilen Fehlercode bis ins UI (Spec
0024), auch in der Serverliste; nur ohne bekannten Code erscheint der rohe
Text. Die feinere Unterscheidung der Ursachen und die Texte stehen in Spec
0069.

## Sicherheitszusagen

- Nach einem fehlgeschlagenen Anlegen bleibt kein Zugangsdaten-Eintrag
  zurück (Fund A2).
- Kein Punkt dieser Spec lockert eine Entscheidung der Filter-Engine oder
  der Bestätigung.
- Ein Startfehler führt nie zu einem spurlosen Verschwinden: mindestens eine
  lesbare Logzeile (Fund B1, B2).

## Grenzen

- Der Fall „Linux ohne Schlüsselbund-Dienst" und der native Startfehler-
  Dialog sind in Spec 0059 und Spec 0101 beschrieben, nicht hier.
