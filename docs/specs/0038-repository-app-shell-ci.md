# Spec 0038 — Repository-Trennung, Anwendungs-Hülle und Erweiterungspunkte

Status: umgesetzt
Zweck: Die Anwendungs-Hülle ist eine Bibliothek, die ein kleines Binary mit einer Verdrahtung startet; Erweiterungspunkte im Backend und im Frontend erlauben einer anderen Edition, Funktionen hinzuzufügen, ohne dass das öffentliche Repo von ihr abhängt.
Bezüge: Spec 0037 (Entitlements), Spec 0090 (CI), Spec 0084 (Anwendungslogik und Hülle), ADR 0035, ADR 0046.

## 1. Ziel

Das öffentliche Repo ist die Grundlage; eine andere Edition hängt von ihm ab,
nie umgekehrt. Dafür ist die Hülle eine Bibliothek, und die Binaries
unterscheiden sich nur in der übergebenen Verdrahtung. Das öffentliche Repo
baut und testet eigenständig, ohne Zugang zu irgendetwas außerhalb davon.
Verhalten ändert sich durch die Aufteilung nicht.

## 2. Hülle und Verdrahtung

Die Hülle bietet einen Startpunkt, der eine **Verdrahtung** (`Wiring`)
erhält. Sie legt fest:

- den Anbieter der Berechtigungen (Spec 0037),
- Listen für KI-Anbieter, Regelquellen und Synchronisations-Umsetzungen,
- zusätzliche Tauri-Plugins (ein Hook auf dem Builder, vor dem Start),
- die Edition (`Community` oder `Official`), die nur für Anzeige und
  Aktualisierung zählt,
- zusätzliche Datenpfade, die die Einstellungen neben den eingebauten
  anzeigen.

Die Verdrahtung der Community Edition hat einen festen Berechtigungs-Satz
ohne freigeschaltete gesperrte Funktion (Spec 0037) und keine Plugins,
Synchronisations-Umsetzungen oder Zusatzpfade. Die Listen für KI-Anbieter
und Regelquellen bleiben dort leer: KI-Anbieter werden je Server-Verbindung
aus dem hinterlegten Schlüssel gebaut, der lokale Regelspeicher erst nach dem
Öffnen der Datenbank angebunden (ADR 0035).

Die Edition darf in Filter, Risiko-Einstufung und Bestätigung nie eine
eigene Verzweigung auslösen.

### Sitzungs-Erweiterung

Die Verdrahtung kann **eine** Sitzungs-Erweiterung enthalten; die Community
Edition hat keine. Sie liefert dem Kern je Server (Server-Kennung inklusive
des lokalen Pseudo-Servers, Gruppenkette von der Wurzel her, Tags):

- **Kontext-Abschnitt:** Text, den der Kern dem KI-Kontext der Sitzung nach
  den eigenen Absätzen anhängt, unter einer eigenen Überschrift (Deutsch und
  Englisch) und in einem eigenen Fence (Spec 0039). Der Text gilt wie eine
  Notiz als nicht vertrauenswürdig: er wird erst redigiert, dann gefenct,
  und setzt wie Notizen das Merkmal „nicht vertrauenswürdiger Inhalt
  eingelesen“. Der Abschnitt wird bei jeder Nutzernachricht neu erfragt. Die Erweiterung
  erhält dabei die Prompt-Sprache der Sitzung (dieselbe, die die Überschrift
  bestimmt) und darf asynchron antworten, etwa nach dem Lesen aus ihrem
  eigenen Datenspeicher.
  Liefert die Erweiterung nichts (oder nur Leerraum) oder gibt es keine
  Erweiterung, ist der Kontext unverändert, ohne leeren Abschnitt.
- **Bekannte Secrets:** Zeichenfolgen, die der Redactor dieses Servers
  zusätzlich zum Sudo-Passwort maskiert: in der Sitzung (Live-Ausgabe,
  gespeicherter Chat, KI-Kontext), beim Wiederverbinden, in Notizen-
  Vorschlägen und im MCP-Notizen-Abruf. Sie gelten nur für den Server, für
  den die Erweiterung sie liefert. Sie werden wörtlich verglichen, nie als
  Regex; leere Werte maskieren nichts. Die Erweiterung kann den Redactor
  offener Sitzungen eines Servers neu aufbauen lassen (etwa nach geänderten
  Werten); schon gesendeter Verlauf wird beim nächsten Senden erneut
  redigiert. Redaction wird dadurch nur erweitert, nie abgeschwächt.
- **Entfernen:** Nach dem Löschen eines Servers (auch eines nicht nutzbaren)
  und nach dem Löschen einer Gruppe, einschließlich jeder mitgelöschten
  Untergruppe, wird die Erweiterung benachrichtigt, damit sie ihre eigenen
  Daten aufräumen kann.

Die Erweiterung hat keinen Zugriff auf den Profilspeicher, nur auf die
übergebenen Angaben. Filter, Risiko-Einstufung und Bestätigung kennen sie
nicht.

### Umwandlung ausgehender Texte

Die Verdrahtung kann eine **Umwandlung ausgehender Texte** enthalten. Sie
ist vertrauenswürdiger Code der Edition und wird mit dem Ziel-Server
(Server-Kennung inklusive des lokalen Pseudo-Servers, Gruppenkette von der
Wurzel her, Tags; dieselben Angaben und dieselbe Bedeutung wie bei der
Sitzungs-Erweiterung: die Kette ist leer für einen Server ohne Gruppe und für
den lokalen Pseudo-Server), der Art des Textes (Kommando oder Dateiinhalt) und dem
Text selbst gefragt. Sie liefert entweder den zu sendenden Text samt der
Liste der Geheimnisse, die sie eingesetzt hat, oder eine Ablehnung mit einer
Meldung. Die Gruppenkette wird beim Senden ermittelt, nicht beim Start der Sitzung;
ein inzwischen verschobener Server liefert also seine aktuelle Kette. Schlägt
die Ermittlung fehl, ist die Kette leer (mit Warnung im Log), und der Text
wird trotzdem vorbereitet. Ohne registrierte Umwandlung (Community) wird der
Text unverändert gesendet, ohne dass die Gruppenkette ermittelt wird.

**Wann sie läuft.** Genau auf diesen Wegen: von der KI vorgeschlagene
Kommandos, von der KI vorgeschlagener Inhalt einer `WriteRemoteFile`-Aktion
und über MCP vorgeschlagene Kommandos. Nie bei Eingaben im Terminal. Sie läuft
**nach** Filter, Risiko-Einstufung, Zweitmeinung und Bestätigung, auf dem
endgültig freigegebenen Text (bei „Bearbeiten und freigeben“ also auf dem
bearbeiteten Text), und unmittelbar vor dem Senden. Die Einsetzung des
gespeicherten Sudo-Passworts (Spec 0018) kommt danach: Sie wirkt auf das
umgewandelte Kommando.

**Rückfrage vor der Bestätigung.** Die Umwandlung kann zusätzlich gefragt
werden, ob sie einen Text ändern wird (reine Abfrage mit Server-Angaben, Art und
ursprünglichem Text; ohne Nebenwirkung, sie sieht und liefert keine Geheimnisse).
Standardantwort: nein. Antwortet sie ja, wird aus einem `AutoExec` für diese
Aktion (Kommando oder `WriteRemoteFile`-Inhalt) ein `Confirm` mit dem Code
`FILTER_INSERTED_SECRET_REQUIRES_CONFIRM` – wie beim gespeicherten
Sudo-Passwort (Spec 0018), auch bei MCP-Herkunft. Der Schritt eskaliert nur: ein
`Confirm` oder `Deny` bleibt unverändert. Er steht in der Eskalationskette
unmittelbar nach dem Sudo-Passwort-Schritt; die Erklärung zur Bestätigung
(Regelvorschlag) nennt denselben Code. Ohne registrierte Umwandlung ändert sich
nichts.

**Struktur-Prüfung.** Ein umgewandeltes Kommando muss, wenn es vom
freigegebenen abweicht, in dieselbe Anzahl Top-Level-Teilkommandos zerfallen
(`|`, `&&`, `||`, `;`, mit dem Parser der Filter-Engine, ohne Zählung von
Command-Substitutionen). Eine andere Anzahl, oder ein umgewandelter Text, den der
Parser nicht zerlegen kann, während der Originaltext zerlegbar war, lehnt ab: es
wird nichts gesendet, die KI erhält eine allgemeine Meldung ohne den
umgewandelten Text. Lässt sich schon das Original nicht zerlegen, wird der
umgewandelte Text nur gesendet, wenn er dem Original gleicht.

**Wer was sieht.** Filter, Risiko-Einstufung, Zweitmeinung, Bestätigungsdialog,
Ledger, Ergebnis-Ereignis, Verlauf und Logs sehen ausschließlich den
ursprünglichen Text. Der umgewandelte Text existiert nur im Aufruf an den
Transport und steht in keiner Anzeige, keinem Verlauf, keinem Ledger und
keiner Log-Zeile; auch die Debug-Ausgabe des Ergebnisses der Umwandlung zeigt
ihn nicht.

**Geheimnisse.** Die gemeldeten eingesetzten Werte werden dem Redactor der
Sitzung hinzugefügt, bevor das Kommando startet. Bereits die Ausgabe genau
dieses Kommandos ist damit maskiert (Live-Ausgabe, Ergebnis, Ledger, Verlauf,
Logs).

**Ablehnung.** Lehnt die Umwandlung ab, wird nichts gesendet und nichts
ausgeführt. Die KI bekommt die Meldung als Ergebnis der Aktion; die Meldung
läuft zuvor durch den Redactor.

**Hard-Blacklist als zweite Absicherung.** Der Kern prüft einen
umgewandelten Befehl noch einmal **nur gegen die Hard-Blacklist** (Spec 0002,
Abschnitt 3; mit denselben Normalisierungen wie dort, ohne Nutzerregeln und
ohne Längenlimit). Trifft sie, wird abgelehnt: Nichts wird gesendet, die KI
bekommt eine allgemeine Meldung, die den umgewandelten Text nicht zitiert.
Das gilt nur, wenn der freigegebene Originaltext die Hard-Blacklist **nicht**
getroffen hat. Hat er sie getroffen, hat die Nutzerin oder der Nutzer ihn
ausdrücklich bestätigt (die Hard-Blacklist führt zu „Bestätigen“); dann wird
der umgewandelte Text gesendet, auch wenn er einen anderen Blacklist-Eintrag
trifft – die Umwandlung ist vertrauenswürdiger Code der Edition.
Darüber hinaus wertet der Kern den umgewandelten Text nicht erneut aus; der
ausgewertete und freigegebene Text bleibt die Grundlage jeder Entscheidung.
Die Umwandlung kann keine Prüfung umgehen oder abschwächen. Für Dateiinhalt
gibt es keine Hard-Blacklist-Prüfung, weil die Blacklist Kommandos beschreibt.

**Master-Passwort-Modus** (Spec 0101): Plugins dürfen Befehle erst **nach
dem Entsperren** registrieren, sonst wären sie vorher aufrufbar.

### 2a. Erweiterungseigene Datenbanktabellen

Eine Erweiterung kann über die Verdrahtung eigene Tabellen in der
verschlüsselten Anwendungsdatenbank ablegen, ohne dass der Kern ihr
Datenmodell kennt und ohne eigene Schlüsselverwaltung. Sie meldet:

- ein **eindeutiges Präfix** der Form `ext_<name>_` (`<name>`: Kleinbuchstaben
  und Ziffern), und
- eine **geordnete Liste eigener Migrationen** (aufsteigende, positive
  Versionsnummern).

Regeln:

- Die Migrationen laufen **nach** den Migrationen des Kerns und **nur nach dem
  Entsperren**: Im Master-Passwort-Modus (Spec 0101) läuft keine
  Erweiterungsmigration, und es wird kein Zugriff herausgegeben, solange die
  App gesperrt ist.
- Jede Erweiterung führt einen **eigenen Migrationsverlauf** (eine Tabelle mit
  ihrem Präfix); Versionen von Kern und Erweiterungen kollidieren nie. Eine
  bereits angewendete Migration läuft nicht erneut; ändert sich eine
  angewendete Migration nachträglich oder fehlt sie in der Liste, startet die
  Erweiterung nicht.
- Eine Migration darf nur Tabellen und Indizes **anlegen, ändern, löschen und
  beschreiben**, deren Name mit dem Präfix beginnt (Indizes nur auf eigenen
  Tabellen). Alles andere schlägt die Migration fehl: Zugriff auf Tabellen
  außerhalb des Präfixes (auch lesend), Sichten, Trigger, virtuelle Tabellen,
  `PRAGMA`, `ATTACH`/`DETACH`, Transaktionssteuerung sowie Fremdschlüssel auf
  Tabellen außerhalb des Präfixes.
- Scheitert eine Migration (abgewiesen oder Datenbankfehler), wird sie
  vollständig zurückgerollt, die Erweiterung **startet nicht**, und der Kern
  startet ohne sie; Kerndaten bleiben unverändert. Das Protokoll nennt nur
  Präfix und Fehlerkategorie, nie SQL-Text oder Werte.
- Nach erfolgreicher Migration erhält die Erweiterung ein Datenbankhandle für
  ihre Tabellen. Es ist kein Sandkasten: Erweiterungscode ist Teil des
  Binaries und wird wie jede andere Verdrahtung vertraut. Erweiterungen
  protokollieren nie Geheimnisse (Spec 0094).
- Der Kern liefert weder eine Erweiterungstabelle noch ein Datenmodell. Ohne
  registrierte Erweiterung ist die Datenbank identisch zu vorher.
- Erweiterungstabellen sind **nicht Teil des Voll-Backups** (Spec 0109, C2).

### Chat-Inhalte von Erweiterungen

Eine Erweiterung kann sich über die Verdrahtung als Quelle für Inhalte
registrieren, die an eine Nutzer-Nachricht gehängt werden (Spec 0110,
Abschnitt 5). Regeln:

- Der Name besteht aus Kleinbuchstaben und Ziffern und ist eindeutig. Eine
  Registrierung mit ungültigem oder doppeltem Namen wird übersprungen und
  ohne Werte protokolliert. Community registriert keine; dann bleiben
  Datenbank, Prompt und Verlauf unverändert.
- Die Erweiterung wählt ihre Quelle aus einer geschlossenen Liste des Kerns
  (zunächst: lokale Datei); eigene Fence-Marker oder ungefencten Text kann
  sie nicht liefern.
- Weil Fremdschlüssel in Kerntabellen verboten sind, meldet der Kern jede
  gelöschte Chat-Sitzung: beim einzelnen Löschen, bei der Aufbewahrungsfrist
  und beim Löschen eines Servers. Der Aufruf kommt erst, nachdem das Löschen
  gelungen ist, genau einmal je entfernter Sitzung.
- Nach den Erweiterungsmigrationen übergibt der Kern jeder Erweiterung einmal
  je Start die Menge der vorhandenen Chat-Sitzungen, damit Reste nach einem
  Absturz zwischen Löschen und Aufruf verschwinden.

### Beobachter der KI-Nutzung

Eine Edition kann über die Verdrahtung beliebig viele Beobachter für die
Nutzung der KI registrieren (Spec 0006, Nutzungsangabe je Anfrage). Regeln:

- Jede abgeschlossene KI-Anfrage einer Sitzung, die eine Nutzungsangabe
  liefert, wird jedem Beobachter genau einmal gemeldet. Das gilt für den
  Haupt-Chat (auch für automatische Folgerunden und Fortsetzungen) und für die
  Nebenaufrufe: Risiko-Zweitmeinung, Einschleusungs-Prüfung, automatischer
  Titel, Notiz-Vorschlag und Zusammenfassung alter Gesprächsrunden. Anfragen
  ohne Sitzung (Modell-Abfrage, Prüfung eines Zugangs) werden nicht gemeldet.
- Die Meldung nennt: gespeicherte Chat-Sitzung und Server (beide leer beim
  lokalen Pseudo-Server; die Chat-Sitzung auch ohne Speicherung), laufende
  Sitzung, Anbieter-Konfiguration, Anbieter-Typ, eingestelltes Modell, Zweck
  der Anfrage, die Nutzungsangabe und den Zeitpunkt. Sie enthält keinen
  Prompt, keine Antwort und kein Kommando.
- Die Beobachter sind rein passiv: Sie geben nichts zurück und beeinflussen
  weder Filter noch Bestätigung noch Ausführung. Sie werden erst nach dem
  Ende der Anfrage und außerhalb des Streamings aufgerufen. Ein langsamer
  oder abstürzender Beobachter verzögert oder stört den Chat nicht; ein
  Absturz wird einmal ohne Inhalt protokolliert und die übrigen Beobachter
  werden trotzdem benachrichtigt.
- Trifft die Nutzungsangabe erst ein, nachdem die Auswertung des Antwort-
  Stroms beendet war, entsteht keine Meldung; der Kern wartet nie darauf.
- Der Kern speichert nichts davon. Community registriert keinen Beobachter;
  dann ändert sich nichts.

## 3. Ein Binary je Edition

Das Community-Binary ist ein dünnes Programm, das nur die Verdrahtung der
Community Edition an den Startpunkt der Hülle übergibt. Ein weiteres Binary
für eine andere Edition gehört nicht zu diesem Repo.

## 4. Frontend-Erweiterungspunkte

Das Frontend hat ein kleines Verzeichnis für Beiträge, die eine Edition
registrieren kann. Jeder Typ hat einen tatsächlichen Renderer:

- **Einstellungs-Abschnitte** (in den Einstellungen gerendert),
- **Dokument-Aktionen** (an der Dokument-Karte im Chat, Spec 0045),
- **Aktionen im Chat-Eingabefeld** (Spec 0110 §10): Symbol-Schaltflächen
  neben „Senden“ in Registrierungsreihenfolge, per Tastatur erreichbar und
  mit zugänglicher Beschriftung; eine Aktion kann gesperrt sein (mit
  Begründung als Tooltip). Eine Aktion fügt dem Eingabefeld ausstehende
  Anhänge hinzu oder entfernt sie. Dazu kann eine Edition einen
  **Ablege-Handler** registrieren, der Dateien bekommt, die auf den Chat
  gezogen werden,
- **Erweiterungen der Erststart-Seite** (Spec 0031 §6),
- **Abschnitte im Server-Formular**: am Ende eines Reiters (Allgemein,
  Verbindung, Sicherheit oder Notizen) im Formular eines bestehenden
  Servers, auch beim lokalen Pseudo-Server (dort nur Allgemein und
  Notizen). Der Registrierende wählt den Reiter; ohne Angabe ist es
  Allgemein. Der Abschnitt erfährt die Server-ID und ob das Formular
  ungespeicherte Änderungen hat. Bei einem neuen, noch nicht gespeicherten
  Server erscheint kein Abschnitt,
- **Reiter in den Server-Einstellungen**: ein eigener Reiter mit
  Beschriftung (Text oder Übersetzungsschlüssel) hinter den eingebauten
  Reitern, sortiert nach einer optionalen Reihenfolge und dann nach
  Registrierungsreihenfolge. Er erscheint nur bei gespeicherten Servern
  (auch beim lokalen Pseudo-Server) und bekommt nur die Server-ID – weder
  den Entwurf des Formulars noch Zugangsdaten. Er wird beim ersten Öffnen
  eingebunden und bleibt eingebunden, solange derselbe Server gewählt ist.
  Wird der aktive Reiter abgemeldet, zeigt die Ansicht wieder Allgemein.
  Eine erneute Registrierung mit derselben Kennung ersetzt den Reiter,
- **Einträge im Sitzungs-Kopf**: ein kleines Element in der Kopfzeile einer
  verbundenen Sitzung, zwischen Status und den Schaltflächen für erneutes
  Verbinden und Trennen, sortiert nach einer optionalen Reihenfolge und dann
  nach Registrierungsreihenfolge. Es erscheint nicht in MCP-Sitzungen. Es
  bekommt nur Kennungen: die ID der laufenden Sitzung, die Server-ID und die
  ID des aktuellen gespeicherten Chats (leer beim lokalen Pseudo-Server und
  bei Sitzungen ohne gespeicherten Chat; sie ändert sich nach „Neuer Chat“),
  weder Zugangsdaten noch Nachrichten. Ein Eintrag lässt sich wieder
  abmelden,
- **Anfrage, einen Chat fortzusetzen**: eine Edition kann die App bitten, einen
  gespeicherten Chat eines Servers fortzusetzen. Das verhält sich genau wie
  die Wahl dieses Chats in der Chat-Auswahl: Hat der Server schon einen
  offenen Tab, wird zu ihm gewechselt und nichts fortgesetzt (Spec 0017 §3);
  sonst gelten der Erststart-Hinweis (Spec 0031), der Verbindungs-Tab mit
  Fortschritt, die Host-Key-Abfrage und „Erneut versuchen“ wie beim
  Fortsetzen aus der Auswahl. Die Anfrage umgeht keine dieser Prüfungen,
- **Abschnitte im Gruppen-Formular**: wie beim Server, nur für bestehende
  Gruppen,
- **Ansichten im Verwalten-Tab**: ein Eintrag mit Beschriftung und Symbol
  als zusätzliche Zeile unter dem Server-Baum der Seitenleiste; ausgewählt
  zeigt er seine Komponente im Hauptbereich. Der Eintrag ist wie die
  Baumzeilen per Tastatur erreichbar.

Ob ein Beitrag gesperrt ist, entscheidet der Registrierende anhand der
Berechtigungen; die Registry kennt sie nicht. Ohne Registrierungen sehen
Formulare, Seitenleiste und Chat-Eingabefeld aus und verhalten sich wie
zuvor.

Die Beiträge werden registriert, bevor die erste Komponente rendert; eine
spätere Registrierung (nach einer asynchronen Prüfung) ist zulässig. Bei
Einstellungs-Abschnitten, Formular-Abschnitten, Server-Reitern, Sitzungs-Kopf-Einträgen, Verwalten-Ansichten und
Chat-Aktionen erscheint sie in einer bereits geöffneten Ansicht ohne Neuladen.

**Berechtigungen im Frontend:** Ein Hook liest den Stand der
Berechtigungen per Befehl und abonniert das Ereignis `entitlements:changed`,
das ausgelöst wird, wenn der Anbieter einen neuen Stand meldet (in der
Community Edition nie, die Infrastruktur steht trotzdem).

**Gesperrte Funktionen:** Ein `FeatureLocked`-Fehler (Spec 0037) wird
zentral abgefangen und als Hinweis-Dialog gezeigt. Der Dialog ist über allen
Ansichten sichtbar, auch bei aktivem Sitzungs-Tab. Ein vollständiger
Upgrade-Ablauf gehört nicht zu diesem Stand.

## 5. Abhängigkeitsprüfung

Die Lizenzprüfung der Rust-Abhängigkeiten (Spec 0090) ist ein Pflichtschritt
der CI.

## 6. CI aus einem frischen Klon

Die CI baut das öffentliche Repo aus einem frischen Klon ohne Geheimnisse:
Bauen, Tests, Clippy ohne Warnungen, Formatprüfung, Lizenzprüfung und
Frontend-Build auf Linux, macOS und Windows (Spec 0090). Schlägt ein
Schritt fehl, weil etwas von außerhalb des öffentlichen Repos fehlt, ist das
ein Fehler in der Trennung.

## 7. Sicherheitszusagen

Die Aufteilung ändert keine Funktion: Jeder Test, der vorher grün war, ist
es danach unverändert.

## 8. Nachweis

Der gesamte Testlauf bleibt unverändert grün; die CI läuft aus einem frischen
Klon durch.

## 9. Grenzen

- Das Frontend-Verzeichnis für Beiträge ist ein gewöhnliches Modul der
  Community-App, kein eigenes Paket (ADR 0046).
- Eine SBOM-Erzeugung und ein vollständiger Upgrade-Dialog gehören nicht zu
  diesem Stand.
