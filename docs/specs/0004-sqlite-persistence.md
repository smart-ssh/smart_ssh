# Spec 0004 — Lokale Persistenz der Server-Profile

Status: umgesetzt
Zweck: Beschreibt, wie Server, Gruppen, Tags und Notizrevisionen dauerhaft in einer lokalen SQLite-Datenbank liegen und welche Zusagen die Speicherung macht.
Bezüge: Spec 0003 (Datenmodell), Spec 0101 (Verschlüsselung der Datenbank, Ablage der Secrets), Spec 0096 (Secrets in der Datenbank), ADR 0010.

## 1. Trennung von der Logik

- Die Kernlogik (Spec 0003, Filter, Risiko, KI) kennt keine Datenbank. Sie
  spricht nur mit einer Speicher-Schnittstelle; die SQLite-Anbindung ist ein
  austauschbarer Baustein dahinter.
- Für Tests gibt es eine Implementierung im Arbeitsspeicher; die Kernlogik ist
  damit ohne Datenbankdatei testbar.

## 2. Migrationen

- Das Schema wird beim Start der Anwendung automatisch angelegt oder auf den
  aktuellen Stand gebracht; der Nutzer führt keinen Migrationsschritt aus.
- Mehrfaches Öffnen derselben Datenbank ist unschädlich; bereits angewendete
  Migrationen werden nicht erneut ausgeführt.
- Fremdschlüssel sind immer aktiv.
- Eine Datenbank, die von einer neueren Programmversion migriert wurde, wird
  von einer älteren Version nicht stillschweigend weiterbenutzt, sondern mit
  einem Fehler abgelehnt.

## 3. Speicherort

Die Datenbank `smart-ssh.db` liegt im plattformüblichen Datenordner des Nutzers:

- macOS: `~/Library/Application Support/Smart SSH/`
- Windows: `%APPDATA%\Smart SSH\`
- Linux: `~/.local/share/smart-ssh/`

Zusätzlich:

- Die Umgebungsvariable `SMART_SSH_DATA_DIR` ersetzt den Ordner (in jedem Build);
  ein leerer Wert gilt als nicht gesetzt.
- Entwicklungs-Builds (`cargo tauri dev`) nutzen einen eigenen Ordner mit
  Suffix „dev", damit sie keine Datenbank eines Release-Builds mit anderem
  Migrationsstand überschreiben oder blockieren.
- Community- und Official-Edition teilen sich denselben Ordner.

## 4. Was gespeichert wird

- **Gruppen** (Name, übergeordnete Gruppe, Notiz, Zeitpunkte).
- **Server** (Felder aus Spec 0003, Abschnitt 3), mit der Anmeldeart als
  Struktur, die ausschließlich Verweise und Pfade enthält, keine Secrets
  (Spec 0003, Abschnitt 4).
- **Tags** je Server als eigene, nach Tag durchsuchbare Zuordnung.
- **Notizrevisionen** (Spec 0003, Abschnitt 5.3) mit Ziel (Server oder Gruppe),
  Inhalt, Urheber, optional Anbieter und Modell, Zeitpunkt.
- Zeitstempel werden als ISO-8601-Text abgelegt und verlustfrei gelesen.

Löschverhalten:

- Wird eine Gruppe gelöscht, werden ihre Untergruppen mitgelöscht; die in ihr
  liegenden Server bleiben erhalten und verlieren nur die Gruppenzuordnung.
- Wird ein Server gelöscht, verschwinden seine Tags; die Gruppe bleibt.
- Wird ein als Jump-Host genutzter Server gelöscht, verliert nur die Zuordnung
  beim abhängigen Server; dieser wird nicht mitgelöscht.
- Vor einem Löschen zeigt die Oberfläche, was mitgelöscht wird und was nur seine
  Zuordnung verliert (Spec 0008).

## 5. Zusagen des Speichers

- Gruppe und Server lassen sich anlegen und unverändert wieder lesen.
- Die Gruppenkette eines Servers wird von der Wurzel bis zur unmittelbaren
  Gruppe geliefert; eine zyklische Kette ergibt einen Fehler.

## 6. Notizen und Revisionen

- Eine neue Notizversion wird zusätzlich zum aktuellen Notizfeld als Revision
  abgelegt, nicht an dessen Stelle.
- Revision und aktuelles Notizfeld ändern sich in **einer** Transaktion: beide
  gelingen oder keine; es gibt nie eine Revision ohne passenden aktuellen Stand
  oder umgekehrt.
- Die Revisionen eines Ziels werden chronologisch gelesen.

## 7. Verschlüsselung

- Die Datenbankdatei ist vollständig verschlüsselt (Spec 0101). Ohne den
  Schlüssel lässt sich keine Tabelle lesen, auch nicht Hostnames, Benutzernamen,
  Gruppen- und Servernamen oder Notizen.
- Secrets (Passwörter, private Schlüssel, Passphrasen, API-Schlüssel) liegen
  in der verschlüsselten Datenbank (Spec 0096, Spec 0101). Im
  Schlüsselbund-Modus liegt dort höchstens noch der Datenbankschlüssel; im
  Master-Passwort-Modus liegt er, mit dem Passwort verpackt, in einer Datei
  neben der Datenbank.
- Konversationsinhalte (Chat, Ledger, Eingabe-Historie, Zusammenfassungen)
  liegen als Klartext innerhalb der verschlüsselten Datei und haben keine
  eigene Feldverschlüsselung (Spec 0036).

## 8. Grenzen

- Es gibt keinen Export und Import von Profilen zwischen Rechnern außer dem
  Import und Export der SSH-Konfiguration (Spec 0075).
- Es gibt keine Synchronisation zwischen Geräten.
- Eine Datenbank, die bei laufender Anwendung kopiert wird, ist nur mit ihrem
  Schlüssel lesbar; ein Schutz vor einer Prozess-Kompromittierung bei
  entsperrter Anwendung ist nicht Ziel der Verschlüsselung.
