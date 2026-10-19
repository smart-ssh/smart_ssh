# Spec 0007 — Desktop-App: Aufbau, Kernschleife und KI-Anbieter-Verwaltung

Status: umgesetzt
Zweck: Die Desktop-App verdrahtet die Module zu einer benutzbaren Oberfläche: Server wählen, verbinden, Chat und Terminal nebeneinander, die KI schlägt Kommandos vor, die Filter-Engine entscheidet, der Nutzer bestätigt, das Ergebnis geht an die KI zurück. Dazu gehört die Verwaltung der KI-Anbieter.
Bezüge: Spec 0002 (Filter-Engine), Spec 0003 (Profile, Notizen, Credential-Ablage), Spec 0004 (Persistenz), Spec 0005 (SSH, Host-Key-Prüfung), Spec 0006 (KI-Anbieter, Redaction), Spec 0008 (Server- und Gruppenverwaltung), Spec 0017 (Tabs und mehrere Sitzungen), Spec 0021 (Fortsetzung nach Aktionsergebnis), Spec 0092 (rotes Risiko verlangt Bestätigung), Spec 0100 (Host-Key-Dialog per Tastatur), ADR 0038 (Ablauf einer unbeantworteten Bestätigung), ADR 0108 (Ende einer Host-Key-Abfrage).

## 1. Ziel

Die App ist der Ort, an dem der Kern-Workflow erlebbar wird: Server auswählen,
verbinden, im Chat mit der KI arbeiten, Vorschläge prüfen und bestätigen, mit
dem Terminal daneben. Die KI ist dabei Copilot, nie ein eigenständig
handelnder Akteur: Nichts erreicht einen Server, ohne durch die Filter-Engine
zu laufen und, wo verlangt, vom Nutzer bestätigt zu sein.

Server- und Gruppenverwaltung (Spec 0008), mehrere parallele Sitzungen
(Spec 0017) und Notiz-Historie sind eigene Bereiche und stehen dort.

## 2. Oberfläche

Die App ist eine Desktop-Anwendung mit einer Web-Oberfläche (React,
TypeScript, Tailwind) und einem eingebetteten Terminal (xterm.js). Die Wahl
des Frameworks ist eine pragmatische Vorgabe ohne Auswirkung auf das
Verhalten.

## 3. Aufbau: Oberfläche und Fachlogik

Die App-Hülle enthält keine Fachlogik. Sie übersetzt Aufrufe der Oberfläche in
Aufrufe der Kernmodule und reicht deren Ergebnisse als reine Datenobjekte
zurück. Asynchrone Abläufe (Chat-Text, Terminal-Ausgabe, Statuswechsel) kommen
als Ereignisse bei der Oberfläche an. Laufende Sitzungen (Verbindung,
KI-Anbieter, Gesprächsverlauf, Filter-Engine) hält die App im Arbeitsspeicher.
Die Aufteilung zwischen Fachlogik und App-Hülle beschreibt Spec 0038.

## 4. Aktionen der Oberfläche

Die Oberfläche kann:

- die Server auflisten und zu einem Server **verbinden**; beim Aufbau kann eine
  Host-Key-Abfrage entstehen, die der Nutzer mit „Vertrauen" oder „Ablehnen"
  beantwortet (Spec 0005, Abschnitt 6);
- für eine Sitzung ein **Terminal** öffnen, Eingaben senden und die Größe
  ändern;
- eine **Chat-Nachricht** senden;
- auf eine wartende Aktion mit **Ausführen**, **Ablehnen** oder
  **Ausführen nach Bearbeiten** (mit geändertem Kommando) antworten;
- die Sitzung **trennen**;
- KI-Anbieter **auflisten, anlegen, ändern, löschen** und einen davon als
  aktiv setzen (Abschnitt 8).

## 5. Ereignisse an die Oberfläche

Die Oberfläche erfährt:

- **Verbindungsstatus:** Wechsel des Status einer Sitzung.
- **Host-Key-Abfrage:** Sitzung, Abfrage, Host und Port, ob der Schlüssel
  unbekannt ist oder vom gespeicherten abweicht, Fingerprint (bei „geändert"
  zusätzlich der erwartete) und der Schlüsseltyp. Der Schlüsseltyp ist der
  Algorithmus des vom Server angebotenen Schlüssels (z. B. `ssh-ed25519`,
  `ecdsa-sha2-nistp256`) oder fehlt, wenn er sich nicht bestimmen lässt. Er ist
  rein anzeigend und beeinflusst die Vertrauensentscheidung nicht.
- **Ende einer Host-Key-Abfrage:** Das Backend wartet nicht mehr auf eine
  Abfrage, weil sie entschieden wurde, abgelaufen ist oder abgebrochen
  wurde. Jede Abfrage hat eine eigene Kennung, damit bei einem erneuten
  Versuch derselben Sitzung nur die passende Abfrage geschlossen wird. Das
  Ereignis ist rein informativ; Ablauf und Abbruch bleiben eine Ablehnung
  (ADR 0108).
- **Terminal-Ausgabe** einer Sitzung.
- **Chat-Text** der KI, in Teilen, sobald er eintrifft.
- **Vorgeschlagene Aktion** mit der Entscheidung der Filter-Engine
  (automatisch ausführen, bestätigen lassen oder blockiert).
- **Ergebnis** einer ausgeführten Aktion.

**Transparenz:** Auch ein von der Filter-Engine blockierter Vorschlag
(`Deny`) wird der Oberfläche gemeldet, nie stillschweigend verworfen. Der
Nutzer sieht, dass die KI etwas vorgeschlagen hat, das blockiert wurde, samt
Grund aus der Entscheidung (Spec 0002). Auch bei „automatisch ausführen"
erscheint der Vorschlag (informativ); die Ausführung läuft dann sofort, ohne
auf eine Antwort zu warten.

## 6. Ablauf eines Kommando-Vorschlags (Kernschleife)

1. Der Nutzer sendet eine Chat-Nachricht.
2. Das Backend baut den Kontext für die KI (einschließlich der wirksamen
   Notizen, Spec 0003) und fragt den KI-Anbieter.
3. Text der KI erscheint sofort im Chat.
4. Schlägt die KI ein Kommando vor, wertet die Filter-Engine es aus
   (Spec 0002).
5. Je nach Entscheidung:
   - **Automatisch ausführen:** Das Kommando läuft sofort über die
     SSH-Verbindung, die Ausgabe wird redigiert (Spec 0006), das Ergebnis
     erscheint im Chat und geht zusätzlich in den Verlauf für die nächste
     KI-Runde.
   - **Bestätigen:** Es wird nichts ausgeführt, bevor der Nutzer antwortet.
   - **Blockiert:** Es wird nichts ausgeführt; der Nutzer wird nur informiert.
6. Ein Notiz-Vorschlag der KI (Spec 0003, Abschnitt 5.2) wartet **immer** auf
   Bestätigung, nie automatisch, unabhängig von der Filter-Engine.

Ein rot eingestufter Vorschlag verlangt zusätzlich Bestätigung (Spec 0092).
Wie nach einem Aktionsergebnis weitergedacht wird, steht in Spec 0021.

**Bestätigung mit Bearbeitung.** Bearbeitet der Nutzer das Kommando vor dem
Ausführen, läuft das geänderte Kommando erneut durch die Filter-Engine. Nur
ein hartes `Deny` überstimmt dann die Freigabe und blockiert das Kommando; ein
erneutes „Bestätigen" nicht, weil der Klick auf „Ausführen" im Dialog bereits
diese Bestätigung ist. Ein bearbeitetes Kommando umgeht die Prüfung nie. Es
gilt im Verlauf als vom Nutzer verfasst. Für Aktionen ohne Kommando (Notiz,
Dateizugriff) gibt es kein Bearbeiten; geht trotzdem eine Bearbeitung ein, läuft
die ursprüngliche Aktion unverändert.

**Ohne Antwort.** Ein Bestätigungsdialog, auf den eine Stunde lang niemand
antwortet, läuft ab und gilt als abgelehnt (ADR 0038).

## 7. Hauptansicht

- Eine **Serverliste** aus der gespeicherten Konfiguration (gruppiert: Spec
  0033). Ein Klick auf einen Server baut die Verbindung auf; bei einem
  unbekannten oder geänderten Host-Key erscheint der Host-Key-Dialog (Spec
  0005, Abschnitt 6; Spec 0100).
- Eine Sitzungsansicht: der **Chat** groß links, das **Terminal** kompakt
  rechts, mit verstellbarer Aufteilung (Spec 0053). Der Chat ist der primäre
  Interaktionskanal, das Terminal dient vor allem der Beobachtung und für
  manuelle Zwischen-Eingriffe.
- Ein **Bestätigungsdialog** für Aktionen, die Bestätigung verlangen: Er zeigt
  das exakte Kommando, erlaubt das Bearbeiten vor der Bestätigung und bietet
  zwei klare Schaltflächen, Ausführen und Ablehnen.
- Die **Einstellungen** mit der Verwaltung der KI-Anbieter (Abschnitt 8).

**Mitlaufen des Chats.** Der Chat verhält sich beim Scrollen so:

- Solange der Nutzer am Ende des Chats ist, läuft die Ansicht mit neuem und
  wachsendem Inhalt mit: gestreamter Antworttext, Aktionskarten,
  Live-Ausgabe laufender Kommandos, Dokumente und Fehlermeldungen. „Am Ende"
  gilt mit derselben kleinen Toleranz wie beim Mitlaufen der Live-Ausgabe
  (Spec 0106, Abschnitt 2.3).
- Scrollt der Nutzer vom Ende weg, hört das Mitlaufen auf: Neuer Inhalt
  verschiebt die Ansicht nicht. Scrollt er zurück ans Ende, läuft der Chat
  wieder mit.
- Solange der Chat nicht am Ende ist, zeigt er über der Eingabe eine runde
  Schaltfläche „Zur neuesten Nachricht springen". Sie hat eine zugängliche
  Beschriftung und einen Tooltip. Ihre Aktivierung (Klick, Enter,
  Leertaste) springt ans Ende und setzt das Mitlaufen fort; am Ende ist die
  Schaltfläche nicht sichtbar.
- Sendet der Nutzer eine eigene Nachricht, springt der Chat immer ans Ende
  und läuft wieder mit.
- Beim Öffnen einer Chat-Sitzung mit Verlauf zeigt der Chat dessen Ende.
- Das automatische Mitlaufen geschieht ohne sichtbaren Sprung und ohne
  Animation.

Mehrere gleichzeitige Sitzungen und ihre Tab-Leiste beschreibt Spec 0017.

## 8. KI-Anbieter-Verwaltung

Die nicht geheimen Teile einer Anbieter-Konfiguration liegen in der lokalen
Datenbank, der API-Key ausschließlich in der Credential-Ablage (Spec 0003),
nie in der Datenbank.

### 8.1 Gespeicherte Konfiguration

Je Anbieter gespeichert sind: Typ, Anzeigename, Basis-URL, Modell, ob
natives Tool-Calling unterstützt wird, der Verweis auf den Key in der
Credential-Ablage und ob der Anbieter aktiv ist. Typen sind `openai`,
`anthropic`, `generic_openai_compatible` und `ollama`. Die Basis-URL ist nur
bei `generic_openai_compatible` und `ollama` relevant. Es ist höchstens ein
Anbieter gleichzeitig aktiv. Weitere Felder und Prüfungen der Anbieter-
Konfiguration beschreiben die Specs 0025, 0050, 0065 und 0072.

### 8.2 Verhalten

- Die Oberfläche bekommt eine Konfiguration **ohne** API-Key zurück. Der Key
  geht nie zurück an die Oberfläche.
- **Anlegen:** Das Backend vergibt eine neue Kennung, leitet daraus den
  Verweis auf den Key ab (`ai-provider:{Kennung}`), speichert den Key in der
  Credential-Ablage und danach erst die übrigen Felder. Scheitert das
  Speichern der Felder, wird der gerade abgelegte Key wieder entfernt.
- **Ändern:** Ein leeres Key-Feld heißt „Key unverändert lassen", nicht „auf
  leer setzen". Zuerst werden die Felder aktualisiert (unbekannte Kennung
  scheitert sauber), nur bei nicht leerem Key wird danach der Key
  überschrieben. Ein Key, der nach dem Trimmen leer ist, zählt als leer
  (Spec 0073).
- **Löschen:** Der Key wird zuerst aus der Credential-Ablage entfernt, dann
  die Konfiguration gelöscht.
- **Der aktive Anbieter lässt sich nicht löschen.** Der Löschversuch scheitert
  mit klarer Fehlermeldung, bevor am Key etwas verändert wird; zuerst muss ein
  anderer Anbieter aktiv gesetzt werden. So entsteht kein impliziter Zustand
  „kein Anbieter aktiv", der gesondert behandelt werden müsste.

### 8.3 Oberfläche

Die Einstellungen zeigen die Liste der Anbieter (Name, Typ, Modell, aktiv oder
inaktiv), ein Formular zum Anlegen (Typ-Auswahl, Name, Modell, Basis-URL nur
bei `generic_openai_compatible` und `ollama`, API-Key als Passwortfeld), eine
Löschen-Schaltfläche je Eintrag und die Wahl des aktiven Anbieters.

Ist kein Anbieter aktiv, steht im Chat statt der Eingabezeile ein Hinweis, den
Anbieter zuerst in den Einstellungen einzurichten, statt eine Eingabe
anzubieten, die ohnehin fehlschlüge. Die Hauptansicht zeigt einen entsprechenden
Hinweis.

## 9. Sicherheitszusagen

- Kein Kommando erreicht einen Server, ohne durch die Filter-Engine gelaufen zu
  sein; auch ein bearbeitetes Kommando wird erneut geprüft (Abschnitt 6).
- Blockierte Vorschläge bleiben sichtbar (Abschnitt 5).
- Ein Notiz-Vorschlag wird nie automatisch übernommen.
- Ein unbekannter oder geänderter Host-Key wird nie ohne ausdrückliche
  Entscheidung des Nutzers akzeptiert (Spec 0005, Abschnitt 6).
- Der API-Key liegt nie in der Datenbank und geht nie zurück an die
  Oberfläche.
