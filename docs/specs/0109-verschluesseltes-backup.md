# Spec 0109 — Verschlüsseltes Voll-Backup (Export und Import)

Status: angenommen (Backup-Schlüssel, Export, Import und Oberfläche umgesetzt, #664, #665, #666)
Zweck: Ein Nutzer sichert alle seine Smart-SSH-Daten in eine einzige verschlüsselte Datei und stellt sie später wieder her, etwa auf einer neuen Installation oder einem zweiten Rechner. Die Wiederherstellung ersetzt immer den lokalen Stand, und nichts geht verloren: Der bisherige Stand wird vorher gesichert und die bisherigen Server landen in einem datierten Ordner.
Bezüge: Spec 0101 (Datenbankverschlüsselung, Master-Passwort, Schlüsselverpackung), Spec 0059 (Datenbank von neuerer Version), Spec 0075 (ssh_config-Import/-Export, eine andere Funktion), Spec 0077 und Spec 0002 (Prüfung von Filterregeln), Spec 0076 (Anmeldeart Schlüsseldatei), Spec 0094 (Logregeln), ADR 0134.
Review-Priorität: ERHÖHT (Kryptografie, Credentials, Filterregeln aus fremder Datei, Parsing nicht vertrauenswürdiger Eingaben, Ersetzen des gesamten Datenbestands)

## 1. Überblick und Begriffe

- **Backup-Datei**: eine einzelne Datei mit dem vollständigen, verschlüsselten Datenbestand (§5). Es gibt genau einen Exportmodus, das **Voll-Backup**.
- **Datenschlüssel**: ein Zufallsschlüssel, der nur den Inhalt einer Backup-Datei verschlüsselt. Jede Backup-Datei hat einen eigenen.
- **Wiederherstellungscode**: ein zufälliger Code mit hoher Entropie, den der Nutzer einmal sieht und aufschreibt. Er ist der Standard, um eine Backup-Datei zu öffnen.
- **Passphrase**: optional anstelle des Codes oder zusätzlich, vom Nutzer gewählt.
- **Schlüsselplatz** (Key Slot): eine Kopie des Datenschlüssels, verpackt unter einem aus Code oder Passphrase abgeleiteten Schlüssel. Eine Datei kann mehrere Schlüsselplätze tragen.
- **Backup-Schlüssel**: der gerätelokale Bestand an Schlüsselmaterial für die Schlüsselplätze (§3), mit dem spätere Exporte ohne erneute Eingabe auskommen.
- Die Backup-Datei ist **nicht** die Datenbankdatei und enthält **nicht** den Datenbankschlüssel (Spec 0101). Sie ist auch **nicht** der ssh_config-Export (Spec 0075).

## 2. Ziel und Nicht-Ziele

**Ziel.** Alles, was den Nutzer ausmacht (Server, Regeln, Notizen, Zugangsdaten, KI-Anbieter, vertraute Host-Keys), lässt sich in eine Datei schreiben und aus ihr vollständig wiederherstellen, ohne dass die Datei ohne Code oder Passphrase etwas preisgibt.

**Nicht-Ziele:**

1. Keine Synchronisation, keine Geräteverwaltung, keine Sequenznummern, keine Löschmarker. Das Format ist versioniert und erweiterbar (§4), mehr wird nicht vorweggenommen.
2. Kein Zusammenführen (Merge). Import ersetzt (§7).
3. Kein teilweiser Export. Es gibt keinen Modus „nur Server" o. Ä.
4. Keine Wiederherstellung bei verlorenem Code und verlorener Passphrase. Es gibt keine Hintertür.
5. Kein Ersatz für den ssh_config-Export (Spec 0075), der unverändert daneben bleibt.

## 3. Backup-Schlüssel und Wiederherstellungscode

- **K1** Der Backup-Schlüssel ist von der Datenbankverschlüsselung getrennt. Er wird aus dem Datenbankschlüssel und dem Wurzelschlüssel (Spec 0101) weder abgeleitet noch umgekehrt. **Der Datenbankschlüssel und der Wurzelschlüssel verlassen das Gerät nie** und stehen in keiner Backup-Datei.
- **K2** **Erster Export.** Gibt es noch keinen Backup-Schlüssel, erzeugt die App ihn und zeigt den Wiederherstellungscode **genau einmal**. Der Export läuft erst weiter, wenn der Nutzer ausdrücklich bestätigt, den Code notiert zu haben. Alternativ setzt der Nutzer an dieser Stelle eine Passphrase (K5); dann gibt es keinen Code.
- **K3** Der Code besteht aus mindestens 128 Bit aus dem Zufallsgenerator des Betriebssystems, dargestellt in Gruppen von je fünf Zeichen eines eindeutigen Alphabets (ohne leicht verwechselbare Zeichen; Groß- und Kleinschreibung und Trennzeichen werden bei der Eingabe nicht unterschieden). Die Eingabe prüft eine Prüfsumme und meldet Tippfehler als solche („Code enthält einen Tippfehler"), ohne zu verraten, welche Datei oder welcher Schlüsselplatz betroffen ist.
- **K4** Der Backup-Schlüssel liegt in der verschlüsselten Datenbank (Spec 0101), auch im Master-Passwort-Modus. Er gehört zum Gerät: Er ist **nicht** Teil der Backup-Datei (§5) und wird durch einen Import **nicht** ersetzt. Spätere Exporte benutzen ihn, der Nutzer braucht dafür keinen neuen Code und tippt nichts ein. Gespeichert wird je Schlüsselplatz der abgeleitete Verpackungsschlüssel samt Typ, Salt und KDF-Parametern (nicht der Code selbst, der nicht abrufbar ist); damit lässt sich der Datenschlüssel jeder neuen Datei in diesen Schlüsselplätzen verpacken, ohne dass Code oder Passphrase erneut eingegeben werden. Der Code selbst ist nach der einmaligen Anzeige nicht mehr abrufbar.
- **K5** **Passphrase.** Mindestens 12 Zeichen, zweimal eingegeben, mit der ausdrücklichen Warnung „ohne Passphrase sind die Backups unlesbar, keine Wiederherstellung" (wie Spec 0101, A13). Sie kann anstelle des Codes oder zusätzlich zu ihm gesetzt werden. Höchstens ein Code und eine Passphrase sind gleichzeitig aktiv.
- **K6** **Neuen Code erzeugen.** Der Nutzer kann jederzeit einen neuen Code erzeugen (Bestätigung wie K2). Neue Dateien sind dann nur noch mit dem neuen Code lesbar; der alte Code ist für sie nutzlos. Bereits geschriebene Dateien bleiben mit dem Code lesbar, mit dem sie geschrieben wurden. Entsprechend ersetzt eine neue Passphrase die alte für neue Dateien.
- **K7** **Umverpacken.** Der Nutzer kann eine vorhandene Backup-Datei mit ihrem alten Code oder ihrer alten Passphrase öffnen und einen neuen Schlüsselplatz hinzufügen oder ersetzen. Dabei wird nur der Datenschlüssel neu verpackt und die Schlüsselplätze im Kopf neu geschrieben (der Kopfkern bleibt unverändert); der verschlüsselte Inhalt bleibt Byte für Byte unverändert. Die Datei wird dabei atomar ersetzt (X3).
- **K8** Code, Passphrase, abgeleitete Schlüssel und Datenschlüssel liegen in Typen, die beim Freigeben überschrieben werden, und verlassen das Backend nie (Spec 0101, A19). Sie stehen in keinem Log und in keiner Fehlermeldung (Spec 0094).

## 4. Dateiformat

Die Datei besteht aus **Kopf** (Klartext, aber authentifiziert) und **Abschnitten** (verschlüsselt). Nichts, was Rückschlüsse auf den Inhalt erlaubt (Namen, Anzahlen, Server, Zeitpunkte des Inhalts), steht im Kopf. Einzige Ausnahme ist der Zeitpunkt der Erstellung der Datei (F1.7), der nur den Export selbst betrifft, nicht den Inhalt.

### 4.1 Kopf

- **F1** Der Kopf besteht aus dem unveränderlichen **Kopfkern** (Punkte 1, 2, 3, 5, 6, 7) und den **Schlüsselplätzen** (Punkt 4). Er enthält ausschließlich:
  1. eine feste **Kennung** (Magic), an der die App die Datei erkennt;
  2. die **Formatversion** (ganze Zahl, beginnt bei 1);
  3. die **KDF**: Algorithmus (`argon2id`) und Parameter (Speicher, Durchläufe, Parallelität). Die Parameter stehen in der Datei und werden nie im Code angenommen;
  4. die **Schlüsselplätze** (F2);
  5. die **Anzahl der Abschnitte** (F4);
  6. optionale **Erweiterungsfelder** (F7);
  7. den Zeitpunkt der Erstellung und die Programmversion, die geschrieben hat (nur zur Anzeige, nicht sicherheitsrelevant).
- **F2** Ein **Schlüsselplatz** enthält: seinen Typ (Code oder Passphrase), ein zufälliges **Salt** (mindestens 16 Byte, je Backup-Schlüssel erzeugt und danach stabil, es ändert sich also nicht von Export zu Export), die Nonce und das Chiffrat des Datenschlüssels. Das Verpacken verwendet XChaCha20-Poly1305 unter dem Argon2id-Ergebnis aus Code bzw. Passphrase und Salt; als zugehörige Daten (AAD) dienen Kennung, Formatversion, KDF-Parameter, Typ und Salt dieses Schlüsselplatzes. Eine Datei hat mindestens einen und höchstens 8 Schlüsselplätze.
- **F3** **KDF-Parameter.** Beim Schreiben gilt mindestens: Argon2id mit m = 64 MiB, t = 3, p = 1 (wie Spec 0101, A14). Beim Lesen werden Parameter **darunter** abgelehnt, ebenso Parameter **über** einer Obergrenze (m > 1 GiB, t > 10, p > 8), damit eine fremde Datei die App nicht durch absurde Kosten lahmlegt. Weil die Parameter in der Datei stehen, können sie später erhöht werden, ohne ältere Dateien unlesbar zu machen.
- **F4** Der Inhalt besteht aus **Abschnitten**. Jeder Abschnitt trägt Typ, Abschnittsversion, ein Kennzeichen **„erforderlich"** und seine Nutzdaten; diese Angaben stehen im verschlüsselten Teil, nicht im Klartext-Kopf. Jeder Abschnitt hat seine eigene zufällige 192-Bit-Nonce.
- **F5** **Authentifizierung.** Der Inhalt jedes Abschnitts wird mit XChaCha20-Poly1305 unter dem Datenschlüssel verschlüsselt. Zugehörige Daten sind der **Kopfkern** (alle Bytes, wie gespeichert; er enthält Kennung, Formatversion, KDF-Parameter, Abschnittsanzahl, Erweiterungsfelder, Erstellzeit und Programmversion, aber **nicht** die Schlüsselplätze), die laufende Nummer des Abschnitts und die Länge des Chiffrats. Damit sind Kopfkern, Reihenfolge und Anzahl der Abschnitte gebunden: Ein geänderter, vertauschter, entfernter oder angehängter Abschnitt und jedes geänderte Byte des Kopfkerns führen zu einem Fehler. Die Schlüsselplätze sind bewusst nicht Teil dieser AAD, damit Umverpacken (K7) den Kopf ändern kann, ohne den Inhalt neu zu verschlüsseln. Jeder Schlüsselplatz ist durch seine eigene AAD (F2) authentifiziert; ein manipulierter oder fremder Schlüsselplatz liefert entweder einen Fehler beim Entpacken oder einen falschen Datenschlüssel, mit dem kein Abschnitt verifiziert. Ein Schlüsselplatz, der zum Öffnen nicht benutzt wird, hat keinen Einfluss auf den Inhalt; er wird nur strikt gelesen (U2).
- **F6** **Wahl des AEAD.** XChaCha20-Poly1305 statt AES-GCM: Die 192-Bit-Nonce darf zufällig gezogen werden, ohne Nonce-Wiederverwendung praktisch befürchten zu müssen; es gibt keine Abhängigkeit von Hardware-AES für Zeit- und Seitenkanaleigenschaften; die Bibliothek ist bereits Abhängigkeit der Verpackung in Spec 0101. Begründung im ADR.
- **F7** **Erweiterbarkeit.** Eine neuere Programmversion darf (a) neue Abschnittstypen und (b) neue Kopf-Erweiterungsfelder hinzufügen, ohne die Formatversion zu erhöhen, solange ältere Leser sie gefahrlos ignorieren können. Jeder Abschnitt und jedes Erweiterungsfeld trägt das Kennzeichen „erforderlich". Ein Leser, der einen **unbekannten erforderlichen** Abschnitt oder ein unbekanntes erforderliches Feld trifft, lehnt die Datei ab wie eine neuere Formatversion (F8). Ein unbekannter **optionaler** Abschnitt oder ein solches Feld wird ignoriert. Weil beides authentifiziert ist, kann es nicht unbemerkt entfernt oder hinzugefügt werden.
- **F8** **Neuere Version.** Eine Datei mit einer Formatversion, die dieses Programm nicht kennt, wird abgelehnt, und es wird **nichts** importiert. Die Meldung nennt beide Versionsnummern („Backup-Format-Version X, dieses Programm kennt Versionen bis Y"), rät, die neueste Version von Smart SSH zu installieren, und führt wie in Spec 0059 zu keinem halben Zustand. Die Formatversion wird vor jeder Ableitung eines Schlüssels geprüft.
- **F9** Die Datei trägt die Endung `.sshbackup`. Die App erkennt sie an der Kennung, nicht an der Endung.

### 4.2 Abschnitte

- **F10** Die Abschnittstypen der Formatversion 1 (alle erforderlich): Server, Gruppen, Schlagworte, Filterregeln, Notizen samt Revisionen, KI-Anbieter, Geheimnisse (Zugangsdaten), vertraute Host-Keys. Nutzdaten sind strikt typisierte, versionierte Strukturen (kein selbstbeschreibendes „beliebiges Objekt").
- **F11** Alle Verweise innerhalb der Datei laufen über die stabilen IDs (UUID v4): Server → Gruppe, Server → Jump-Host, Regel → Server/Schlagwort, Notiz → Server, Geheimnis → Besitzer. Sie werden beim Import mit ihren IDs übernommen (§7).
- **F12** **Grenzen** (jede Überschreitung lehnt die ganze Datei ab, §8):

| Grenze | Wert |
|---|---|
| Dateigröße | 128 MiB |
| Kopfgröße | 64 KiB |
| Größe eines Abschnitts (Chiffrat) | 64 MiB |
| Abschnitte je Datei | 64 |
| Schlüsselplätze | 8 |
| Server | 10 000 |
| Gruppen | 10 000 |
| Filterregeln | 50 000 |
| Notizrevisionen insgesamt | 200 000 |
| Geheimnisse | 50 000 |
| Länge eines einzelnen Texts (Name, Muster, Pfad …) | 64 KiB |
| Länge eines Geheimnisses oder einer Notiz | 4 MiB |
| Verschachtelungstiefe der Strukturen | 32 |
| Tiefe des Gruppenbaums | 64 |

Es gibt keine Kompression im Format; ein kleines Chiffrat kann also nicht zu einem viel größeren Klartext aufquellen.

## 5. Inhalt

- **C1** **Enthalten** (Voll-Backup): Server mit ihren stabilen IDs und allen ihren Einstellungen, Gruppen (Ordner), Schlagworte, Filterregeln, Notizen **einschließlich der Notizrevisionen**, KI-Anbieter-Konfigurationen, **alle Zugangsdaten** (Server-Passwörter, private Schlüssel, Passphrasen, KI-Schlüssel) und die vertrauten Host-Keys. Die Markierung „Kopie" (Spec 0103, §3a) eines Servers oder einer Gruppe wird als optionales Feld der Zeile mitgeführt (F7): fehlt es, gilt „keine Kopie"; ein Leser, der es nicht kennt, ignoriert es.
- **C2** **Nicht enthalten**: Chat-Sitzungen, Sitzungs-Caches, Prompt-Verlauf, Kommando-Ledger, die Geräteeinstellungen (Sprache, Layout, MCP-Einstellungen), Bearbeitungskopien, der Backup-Schlüssel (K4), der Datenbank- und Wurzelschlüssel (K1), der lokale Pseudo-Server, **die Tabellen von Erweiterungen** (Präfix `ext_<name>_`, Spec 0038 §2a), der Schlüssel-Wert- und Secret-Speicher für Integrationen und alles, was nicht in der Datenbank oder bei den Host-Keys liegt.
- **C3** **Schlüsseldatei-Anmeldung** (Spec 0076): Exportiert wird nur der **Pfad**, nie der Inhalt der Schlüsseldatei. Importierte Schlüssel (Spec 0076, C-8) wandern als gespeicherte Schlüssel, Pfad-Schlüssel wandern als Pfad. Gibt es die Datei auf dem Zielgerät nicht, zeigen Serverliste und Formular den Zustand **„Schlüsseldatei auf diesem Gerät nicht gefunden"** (mit dem Pfad), und ein Verbindungsversuch scheitert mit genau dieser Meldung. Es gibt keinen stillen Fehler und keinen Rückfall auf eine andere Anmeldeart. Der Import selbst öffnet diese Datei nicht (§10, S-5).
- **C4** Die Datei enthält alle Geheimnisse, aber nie im Klartext (§4.1, Sicherheitszusagen S-1).

## 6. Export

- **X1** Der Export steht an einer eigenen Stelle der Einstellungen unter dem Namen **„Verschlüsseltes Backup"** (§9). Er verlangt die Wahl des Zielorts durch den Nutzer (Dateidialog des Systems, der auch das Überschreiben erfragt).
- **X2** Der Export liest den Datenbestand in **einem konsistenten Schnitt** (eine Lesetransaktion): Die Datei zeigt einen Zeitpunkt, keinen Mix aus zwei Ständen.
- **X3** **Atomares Schreiben.** Die Datei wird zuerst in eine **temporäre Datei im selben Verzeichnis** wie das Ziel geschrieben (neu angelegt, nur für den Nutzer lesbar, unter Unix 0600), vollständig geleert (Flush bis auf den Datenträger) und erst dann auf den Zielnamen **umbenannt**. Bricht der Vorgang ab (Fehler, Absturz, Beenden der App), liegt am Ziel **keine halbe Datei**, und die temporäre Datei wird entfernt, soweit der Prozess noch läuft. Eine bereits vorhandene Zieldatei bleibt bis zum erfolgreichen Umbenennen unverändert. Dieser Mechanismus gilt für alle Schreibvorgänge der Backup-Funktion (Export, Sicherung vor dem Import, Umverpacken K7).
- **X4** Nach dem Schreiben **liest die App die Datei zurück**, öffnet sie mit dem Datenschlüssel und prüft die Authentifizierung aller Abschnitte; erst dann meldet sie Erfolg und zeigt den Pfad. Schlägt die Prüfung fehl, meldet sie einen Fehler und entfernt die Datei.
- **X5** Der Export erzeugt je Datei einen **neuen** Datenschlüssel und neue Nonces und verpackt ihn unter den aktiven Schlüsselplätzen des Backup-Schlüssels (K4). Es gibt kein Zurücksetzen oder Wiederverwenden eines Datenschlüssels.
- **X6** Der Export ändert den Datenbestand nicht. Er wird im Master-Passwort-Modus nur im entsperrten Zustand angeboten (Spec 0101, A16).

## 7. Import

Der Import ersetzt den lokalen Datenbestand **immer vollständig**. Es gibt kein Zusammenführen. Er läuft bei offener, entsperrter Datenbank (auch im Master-Passwort-Modus).

- **I1** **Öffnen.** Der Nutzer wählt eine Datei und gibt den Code oder die Passphrase ein. Die App prüft die Kennung, die Größe und die Formatversion (F8), leitet danach mit den Parametern aus der Datei den Schlüssel ab, öffnet die Schlüsselplätze der Reihe nach und entschlüsselt und prüft **alle** Abschnitte vollständig im Speicher (§8). Bis hierher wurde am lokalen Bestand **nichts** verändert. Ein falscher Code, eine falsche Passphrase und eine beschädigte Datei sind **nicht unterscheidbar**; die Meldung lautet „Code oder Passphrase falsch oder Datei beschädigt" (wie Spec 0101, A17).
- **I2** **Leerer Bestand.** Ist der lokale Bestand leer (kein Server, keine Gruppe, keine Regel, keine Notiz, kein KI-Anbieter von Hand angelegt; der lokale Pseudo-Server zählt nicht), wird **direkt** importiert: ohne Zählung, ohne Sicherung, ohne Ordner „Alter Stand".
- **I3** **Zählung und Bestätigung.** Ist lokal etwas vorhanden, zeigt die App die Zahlen aus der geprüften Datei neben den lokalen, etwa „ersetzt 12 Server / 30 Regeln / 8 Notizen durch 15 / 42 / 11" (zusätzlich Ordner und KI-Anbieter), und verlangt eine ausdrückliche Bestätigung. Ohne Bestätigung ändert sich nichts. Die Zahlen stammen aus dem tatsächlich zu importierenden Inhalt, nicht aus Angaben im Kopf.
- **I4** **Ablauf nach der Bestätigung**, in dieser Reihenfolge:
  1. **Sicherung des aktuellen Stands.** Die App schreibt ein vollständiges verschlüsseltes Backup des **jetzigen** Stands, mit demselben Code wie der Export (§6) und dem Backup-Schlüssel des Geräts, und zeigt den Pfad. Hat das Gerät noch keinen Backup-Schlüssel, läuft zuerst K2. Der Zielort ist ein Standardverzeichnis (dem Nutzer vorher gezeigt, wählbar); die Datei trägt Datum und Uhrzeit im Namen und überschreibt keine vorhandene Datei. Gelingt die Sicherung nicht (einschließlich der Rückleseprüfung X4), **bricht der Import ab**, und nichts ist verändert.
  2. **Ordner „Alter Stand <Datum>"** (englisch „Previous data <date>", Datum als `JJJJ-MM-TT` nach lokaler Zeit; existiert der Name schon, folgt ` (2)`, ` (3)` …). Die bisherigen **Server und ihre Gruppenstruktur** werden in diesen neuen Ordner verschoben. Die verschobenen Server und Gruppen erhalten die Markierung „Kopie" (Spec 0103, §3a), die der Nutzer einzeln aufheben kann; sonst setzt nichts die Markierung. Dabei erhalten die verschobenen Server und Gruppen **neue IDs**, damit sie nie mit importierten Einträgen gleicher ID kollidieren oder verwechselt werden. Verweise zwischen ihnen (Jump-Host, Elterngruppe) zeigen auf die neuen IDs. Was einem alten Server gehört (Zugangsdaten, Notizen samt Revisionen, auf ihn bezogene Regeln), zieht mit der neuen ID um, damit die alten Server weiter benutzbar bleiben.
  3. **Import** des Inhalts mit den **IDs aus der Datei**. Regeln, die nicht an einen Server gebunden sind, KI-Anbieter und Host-Keys des alten Stands werden durch die der Datei ersetzt; sie stehen danach nur noch in der Sicherung aus Schritt 1.
- **I5** **Atomar.** Schritt 2 und 3 laufen in **einer** Transaktion der Datenbank: Entweder ist der gesamte Import angewandt, oder der lokale Stand ist unverändert. Die Host-Keys liegen außerhalb der Datenbank; sie werden in derselben Transaktion als „ausstehend" vorgemerkt und unmittelbar nach dem erfolgreichen Abschluss atomar (X3) in ihre Datei geschrieben. Wird die App dazwischen beendet, schließt der nächste Start die Übernahme ab, **bevor** irgendeine Verbindung aufgebaut wird. Bis dahin gelten die alten Host-Keys; eine Abweichung führt zu einer Nachfrage, nie zu stillem Vertrauen. Die Sicherung aus Schritt 1 bleibt in jedem Fall liegen.
- **I6** Die Sicherung aus Schritt 1 lässt sich ihrerseits importieren und stellt den Stand vor dem Import wieder her (mit ihrem eigenen Ordner „Alter Stand", falls dann lokal etwas steht).
- **I7** Der Ordner „Alter Stand" wird **nie automatisch** aufgeräumt oder gelöscht. Der Nutzer behandelt ihn wie jeden anderen Ordner.
- **I8** Nach erfolgreichem Import zeigt die App eine Zusammenfassung (Zahlen, Pfad der Sicherung, Name des Ordners „Alter Stand") und lädt alle Ansichten neu. Offene Sitzungen zu ersetzten Servern werden vor dem Ersetzen vollständig geschlossen — über denselben Schließweg wie „Trennen“ (erhöhter Kanal widerrufen, Pfadfreigaben entfernt, wartende Bestätigung abgelehnt, laufender KI-Befehl abgebrochen und abgewartet) —, nachdem der Nutzer das in I3 bestätigt hat (die Bestätigung nennt offene Sitzungen).
- **I9** Der Import ändert den Backup-Schlüssel (K4) und die Geräteeinstellungen (C2) nicht.

## 8. Nicht vertrauenswürdige Eingabe

Eine Backup-Datei ist **nicht vertrauenswürdige Eingabe**, auch wenn sie von der App selbst stammt.

- **U1** **Integrität vor Wirkung.** Bevor irgendetwas angewandt wird, sind Authentizität und Integrität aller Abschnitte und des Kopfkerns bestätigt (F5). **Ein geändertes Byte** an irgendeiner Stelle des Kopfkerns oder der Abschnitte lehnt die **ganze** Datei ab; es wird nie etwas teilweise importiert.
- **U2** **Strenges Lesen.** Kopf und Abschnitte werden strikt gelesen: unbekannte Felder in bekannten Strukturen, doppelte Schlüssel, falsche Typen, Zahlen außerhalb des Wertebereichs, ungültiges UTF-8, ungültige UUIDs, überlange Texte und zu tiefe Verschachtelung sind Fehler. Längenangaben werden gegen die tatsächliche Dateigröße und gegen die Grenzen (F12) geprüft, **bevor** Speicher für sie reserviert wird.
- **U3** **Grenzen** (F12) gelten vor und während des Lesens. Eine überschrittene Grenze lehnt die Datei mit verständlicher Meldung ab; nichts wird angelegt.
- **U4** **Kein Absturz.** Kein Eingabeinhalt darf zu einem Panic, einem Überlauf oder unbegrenztem Speicher- oder Zeitverbrauch führen. Alle Fehler sind Meldungen. Der Schlüsselableitungsaufwand ist durch F3 nach oben begrenzt.
- **U5** **Referentielle Prüfung** vor dem Anwenden: keine doppelten IDs, jeder Verweis (F11) trifft einen vorhandenen Eintrag, keine Schleife im Gruppenbaum oder in Jump-Host-Ketten, Geheimnisse gehören zu vorhandenen Besitzern. Ein Verstoß lehnt die ganze Datei ab.
- **U6** **Filterregeln.** Jede importierte Regel durchläuft **dieselbe Prüfung wie eine vom Nutzer angelegte** (Spec 0077, 3.1, und die Regelverwaltung nach Spec 0009 / Spec 0002): gültiges Muster, bekannte Aktion und bekannter Geltungsbereich, erlaubte Längen. Eine ungültige Regel lehnt die **ganze Datei** ab; sie wird nicht übersprungen, weil das Weglassen einer Deny- oder Confirm-Regel eine Abschwächung wäre. Die Datei kann **keine** eingebaute Sicherung (Hard-Blacklist, Risiko-Einstufung, Reihenfolge der Auswertung) ausschalten oder umgehen, weil diese nicht Daten der Datei sind, und enthält kein Feld, das dies ausdrücken könnte. Auch alle übrigen Werte (Einstellungen von Servern wie Risiko-, Nachprüfungs- oder Injection-Einstellungen) müssen bekannte, gültige Werte sein.
- **U7** Pfade und Namen aus der Datei werden wie jede fremde Eingabe behandelt: Der Import öffnet aus ihnen keine Datei (C3), führt nichts aus und baut keine Verbindung auf.

## 9. Trennung von ssh_config-Export

- **T1** Der verschlüsselte Export und der ssh_config-Export (Spec 0075) sind nicht zu verwechseln: eigene Namen, eigene Stellen, eigene Dialogtitel. Das Backup heißt „Verschlüsseltes Backup" / „Encrypted backup" und liegt unter „Einstellungen → Daten und Backup". Der ssh_config-Export behält seinen Namen und Ort. Kein Dialog benutzt für beide denselben Titel oder dieselbe Beschriftung.
- **T2** Der ssh_config-Export zeigt den Hinweis: „Enthält keine Zugangsdaten, Regeln oder Notizen – für ein vollständiges Backup den verschlüsselten Export nutzen." (englisch: „Contains no credentials, rules or notes. For a complete backup, use the encrypted backup."). Siehe Spec 0075, 3.2.8.
- **T3** Die Dateiendung (F9) und die Erkennung an der Kennung verhindern, dass eine Backup-Datei als ssh_config gelesen wird und umgekehrt; der jeweils falsche Import lehnt die Datei mit einem Hinweis auf die richtige Funktion ab.
- **T4** Der Dialog des Backup-Exports sagt vor dem Schreiben, was enthalten ist (auch Zugangsdaten, verschlüsselt) und was nicht (C2).
- **T5** Die Texte stehen auf Deutsch und Englisch mit gleicher Aussage; sie nennen nie ein Geheimnis.

## 10. Sicherheitszusagen

- **S-1** **Kein Klartext-Geheimnis in der Datei.** Weder ein Passwort noch ein Schlüssel noch ein KI-Schlüssel noch ein Wert, den die Ausgabe-Redaktion (Redaktion nach Spec 0036 und Spec 0094) erkennen würde, noch ein Servername steht im Klartext in der Datei. Der Kopf enthält nur Verfahrensdaten (F1).
- **S-2** **Strenge Ablehnung.** Eine veränderte, abgeschnittene, erweiterte oder umgeordnete Datei wird als Ganzes abgelehnt; nichts wird teilweise angewandt (U1, F5).
- **S-3** **Keine Schwächung von Filterregeln.** Ein Import kann den Schutz nie schwächen (U6). Es gibt nur Verschärfungsrichtung: nichts in der Datei kann eine Regel gültig machen, die der Nutzer nicht anlegen könnte.
- **S-4** **Der Datenbankschlüssel verlässt das Gerät nie.** Er und der Wurzelschlüssel stehen in keiner Datei und werden nicht aus ihr gewonnen (K1).
- **S-5** **Keine Dateizugriffe aus fremden Pfaden.** Der Import öffnet keine Schlüsseldatei und keinen Pfad aus der Datei (C3, U7).
- **S-6** **Geheimnisse bleiben im Speicher geschützt.** Schlüssel, Code, Passphrase und entschlüsselter Inhalt werden nach Gebrauch überschrieben (K8). Der entschlüsselte Inhalt wird nie auf den Datenträger geschrieben (kein temporärer Klartext).
- **S-7** **Logregeln.** Im Log stehen nur Ereignis, Ergebnis und Zahlen, nie Code, Passphrase, Datei-Inhalt, Pfade aus der Datei oder Servernamen (Spec 0094).
- **S-8** **Keine Hintertür.** Es gibt keinen zweiten Weg zum Datenschlüssel außer den Schlüsselplätzen der Datei.

## 11. Grenzen

- Verlorener Code und verlorene Passphrase heißen: Die Dateien sind unlesbar. Das sagen K2 und K5 vor dem Bestätigen.
- Die Datei schützt nur so gut wie der Code bzw. die Passphrase. Ein schwache Passphrase (nur das Minimum aus K5) ist angreifbar durch Raten; der Argon2id-Aufwand (F3) bremst, verhindert aber nichts.
- Die Datei ist ein Stand zu einem Zeitpunkt. Wer nach dem Export Daten ändert, hat sie in der Datei nicht.
- Beim Import werden die vertrauten Host-Keys ersetzt, nicht zusammengeführt. Alte Server im Ordner „Alter Stand" können deshalb bei der nächsten Verbindung die Host-Key-Nachfrage auslösen; die Sicherung enthält die alten Host-Keys.
- Die Datei sichert keine Dateien außerhalb der Datenbank außer den Host-Keys; Schlüsseldateien auf der Platte gehören dem Nutzer (C3).
- Die ungenutzten Sync-Begriffe der Kernbibliothek (verschlüsseltes Datenpaket, Sync-Quittung, Sync-Backend) gehören zu einem künftigen Abgleich und werden **nicht** wiederverwendet; das Backup definiert eigene Typen. Sie bleiben unverändert, bis ein Sync-Thema sie braucht (ADR 0134).

## 12. Testfälle

Pflicht, weil ERHÖHT. Kennungen sind stabil.

1. **Kein Klartext.** Ein Datenbestand mit Geheimnissen aller Art (Passwort, privater Schlüssel im PEM-Format, KI-Schlüssel der bekannten Formen, ein `$argon2id$…`-Hash, ein Server mit auffälligem Namen) wird exportiert. Die Bytes der Datei enthalten **kein** Muster des Redaktors (die Muster des Redaktors laufen über die Datei als Prüfung) und keinen der Namen und Geheimnisse als Teilzeichenfolge. Gegenprobe: Dieselbe Prüfung über einen absichtlich unverschlüsselten Export des Bestands schlägt an.
2. **Rundlauf auf zweiter Installation.** Export auf Installation A, Import auf der leeren Installation B: Server (mit gleichen IDs), Ordner, Schlagworte, Regeln, Notizen samt Revisionen, Zugangsdaten und Host-Keys sind gleich. Ein Server mit Passwort kann mit dem wiederhergestellten Passwort verbinden (gegen den lokalen Testserver). Chat-Sitzungen, Ledger und Geräteeinstellungen sind **nicht** übernommen.
3. **Ein Byte geändert.** Jede Position der Datei (Kennung, jedes Kopfkernfeld, der zum Öffnen benutzte Schlüsselplatz, Abschnitts-Rahmen, Chiffrat, Tag) wird einzeln geändert: Der Import wird abgelehnt, der lokale Bestand ist byteweise unverändert, nichts ist teilweise importiert. Dasselbe für Abschneiden, Anhängen, Vertauschen zweier Abschnitte, Entfernen eines Abschnitts, Zusammenfügen von Kopfkern und Abschnitten aus zwei Dateien. Ein geändertes Byte in einem **nicht benutzten** Schlüsselplatz, der strikt lesbar bleibt, ändert nichts am Ergebnis (F5); ist er nicht mehr strikt lesbar, wird die Datei abgelehnt.
4. **Neuere Version.** Eine Datei mit höherer Formatversion wird abgelehnt, bevor ein Schlüssel abgeleitet wird; die Meldung nennt beide Versionen und rät zur Aktualisierung; nichts wird importiert. Ein unbekannter **erforderlicher** Abschnitt wird ebenso abgelehnt; ein unbekannter **optionaler** Abschnitt wird ignoriert, der Rest importiert.
5. **Abbruch beim Schreiben.** Der Schreibvorgang wird an jeder Stelle unterbrochen (Fehler des Dateisystems beim Schreiben, Leeren und Umbenennen; Abbruch des Prozesses). Am Ziel liegt **keine** halbe Datei; war dort schon eine Datei, ist sie unverändert; die temporäre Datei ist entfernt. Gegenprobe: ein Schreiben direkt in die Zieldatei würde dieselbe Prüfung nicht bestehen.
6. **Import über vorhandene Daten.** Bestand lokal mit 12 Servern, 30 Regeln und 8 Notizen, Datei mit 15 / 42 / 11: Die Zählung zeigt genau diese Zahlen. Nach Bestätigung existiert die Sicherung, **lässt sich selbst importieren** und liefert den alten Stand; die 12 alten Server stehen im Ordner „Alter Stand <Datum>" mit **neuen IDs** und ihren Zugangsdaten; die 15 neuen tragen die IDs aus der Datei. Bricht ein Schritt in der Transaktion ab, ist der Stand wie vor dem Import.
7. **Fehlende Schlüsseldatei.** Ein Server mit Schlüsseldatei-Pfad, der auf der Zielmaschine nicht existiert, zeigt „Schlüsseldatei auf diesem Gerät nicht gefunden"; die Verbindung scheitert mit dieser Meldung; der Import hat die Datei nie geöffnet (geprüft durch einen Pfad, der auf eine nicht lesbare Datei zeigt).
8. **Leerer Bestand.** Import in eine leere Installation: keine Zählung, keine Sicherung, kein Ordner „Alter Stand".
9. **Falscher Code / beschädigte Datei.** Beide ergeben dieselbe Meldung. Tippfehler im Code (Prüfsumme) ergibt die Tippfehler-Meldung.
10. **Mehrere Schlüsselplätze.** Datei mit Code und Passphrase lässt sich mit jedem von beiden öffnen. Nach K6 (neuer Code) lässt sich eine neue Datei mit dem alten Code nicht öffnen, eine alte Datei weiter. Umverpacken (K7) lässt das Chiffrat und den Kopfkern byteweise unverändert; die Datei lässt sich danach mit dem neuen Zugang öffnen, und ein Schlüsselplatz aus einer anderen Datei bleibt wirkungslos.
11. **KDF-Grenzen.** Parameter unter dem Minimum und über der Obergrenze werden vor der Ableitung abgelehnt; eine Datei mit höheren, aber zulässigen Parametern ist lesbar.
12. **Filterregeln.** Eine Datei mit einer ungültigen Regex-Regel (Deny), mit unbekannter Aktion und mit überlangem Muster wird als Ganzes abgelehnt. Eine gültige Regel verhält sich nach dem Import in der Auswertung genau wie vor dem Export (gleiche Entscheidung für eine Reihe von Kommandos). Gegenprobe: Würde die Validierung ausgelassen, kommt die ungültige Regel durch.
13. **Strenges Lesen und Grenzen.** Doppelte Schlüssel, unbekannte Felder, ungültige UUID, Verweis auf einen fehlenden Server, Schleife in Jump-Host-Kette und im Gruppenbaum, doppelte ID, Längen und Zahlen über F12, eine „Zip-Bomb"-artige Längenangabe, Verschachtelung über 32: jeweils abgelehnt ohne Panic und ohne nennenswerten Speicher; ein Fuzz-Lauf über Kopf und Abschnitts-Rahmen stürzt nie ab.
14. **Master-Passwort-Modus.** Export und Import laufen im entsperrten Master-Passwort-Modus; im gesperrten Zustand sind sie nicht erreichbar. Der Backup-Schlüssel ist nach Neustart noch da, in keiner Backup-Datei.
15. **Backup-Schlüssel getrennt.** Weder Datenbank- noch Wurzelschlüssel noch Backup-Schlüssel stehen in der Datei; ein Import ändert den Backup-Schlüssel nicht.
16. **Host-Keys atomar.** Wird der Prozess nach dem Commit, aber vor dem Schreiben der Host-Key-Datei beendet, schließt der nächste Start die Übernahme ab, bevor eine Verbindung entsteht.
17. **UI-Trennung.** Der Hinweis im ssh_config-Export erscheint auf Deutsch und Englisch; Titel und Ort beider Funktionen unterscheiden sich; eine Backup-Datei im ssh_config-Import und umgekehrt wird abgelehnt.
18. **Erster Export.** Ohne Bestätigung „Code notiert" wird nichts geschrieben; der Code ist danach nicht mehr abrufbar; eine Passphrase unter 12 Zeichen wird abgelehnt.
19. **Logs.** Nach Export und Import enthält das Log weder Code noch Passphrase noch Inhalte.

## 13. Klarstellungen

- Die Wahl der Verfahren (Argon2id, XChaCha20-Poly1305) und der Aufbau (Datenschlüssel + Schlüsselplätze) sind im ADR 0134 begründet; die Spec legt Verhalten und Prüfpunkte fest.
- „Alle Zugangsdaten" heißt: alles, was die Datenbank als Geheimnis führt, mit Ausnahme des Backup-Schlüssels selbst.
- Umsetzungsstand: Das Dateiformat und die Kryptografie nach §4 (Kopf, Schlüsselplätze, Abschnitte, Wiederherstellungscode K3, Umverpacken K7, strenges Lesen U1–U4, Grenzen F12 soweit sie das Format betreffen) setzt das Modul `crates/core/src/backup/` um, das atomare Schreiben nach X3 der Helfer `crates/core/src/atomic_write.rs`. Die typisierten Abschnittsinhalte (F10), Export, Import und Oberfläche sind umgesetzt (siehe unten); der Status bleibt „angenommen“, bis Export und Import gemergt sind.
- Eine Datei hat mindestens einen Abschnitt: Erst die Abschnitte authentifizieren den Kopfkern.
- Umsetzungsstand Export (#664): Der Backup-Schlüssel (K1, K2, K4–K6) liegt als reservierter Eintrag im Geheimnisspeicher der verschlüsselten Datenbank, ohne Schemaänderung; er erscheint in keiner Geheimnisliste, nicht im Voll-Backup und wird vom Umzug und Aufräumen der Geheimnisse nie angefasst. Der Export liest alle Tabellen nach C1 in einer Lesetransaktion und die vertrauten Host-Keys im selben Aufruf, schreibt atomar (X3), liest zurück und prüft alle Abschnitte unter jedem aktiven Schlüsselplatz (X4). Ohne Backup-Schlüssel meldet der Export, dass der Ablauf des ersten Exports (K2) nötig ist, und legt nichts an. Befehle für Schlüsselstatus, Schlüssel mit Code anlegen, Passphrase setzen, neuen Code erzeugen und Export sind nur bei entsperrter Datenbank erreichbar; die Oberfläche beschreibt der folgende Absatz zur Oberfläche (#666). Die Protokolle nennen nur Ereignis, Ergebnis und Anzahlen. Die Testfälle 1, 14 (Export-Teil), 15, 18 und 19 (Export-Teil) sind automatisiert.
- Umsetzungsstand Import (#665): Die Anwendungslogik öffnet und prüft die ganze Datei im Speicher (§7 I1, §8 U1–U7), bevor sie etwas ändert. Bei leerem Bestand importiert sie sofort; sonst liefert sie die Zahlen beider Seiten (Server, Ordner, Regeln, Notizen, KI-Anbieter, betroffene offene Sitzungen) und ändert erst nach einem zweiten, bestätigenden Aufruf etwas: Sicherung des Stands mit dem Backup-Schlüssel des Geräts in ein Standardverzeichnis (Dateiname mit Datum und Uhrzeit, nie überschreibend; scheitert sie oder fehlt der Backup-Schlüssel, bleibt alles unverändert), dann Ordner „Previous data <Datum>“ (bei Namensgleichheit mit ` (2)`, ` (3)` …) mit den alten Servern und Ordnern unter neuen IDs samt ihrer Zugangsdaten, Notizen und serverbezogenen Regeln, dann der Import mit den IDs der Datei, beides in einer Transaktion. Die Host-Keys werden in dieser Transaktion als ausstehend vorgemerkt und gleich nach dem Abschluss geschrieben; ein verbliebener Vermerk wird beim nächsten Start vor jeder Verbindung abgeschlossen. Als Zugangsdaten der Datei gelten nur die von Servern und KI-Anbietern, und jede Referenz eines Servers muss auf dessen eigene Zugangsdaten zeigen. Offene Sitzungen schließt der Aufrufer nach der Bestätigung und vor dem Ersetzen. Die Testfälle 2, 6, 7, 8, 12, 16 und 19 (Import-Teil) sind automatisiert.
- Umsetzungsstand Oberfläche (#666): Unter „Einstellungen → Daten und Backup" steht der Abschnitt „Verschlüsseltes Backup" (breit und schmal, Knöpfe mindestens 44 px hoch, Dialoge im schmalen Fenster füllschirmig). Er nennt, was enthalten ist und was nicht (T4), und bietet „Backup exportieren", „Aus Backup wiederherstellen", „Neuer Wiederherstellungscode" und „Passphrase festlegen". Der erste Export zeigt den Code einmal, in Fünfergruppen mit Kopierknopf; „Weiter" bleibt aus, bis „Ich habe den Code notiert" gesetzt ist, und der Code ist danach nicht mehr abrufbar. Wird der Dialog ohne diese Bestätigung verlassen, verlangt der nächste Export einen neuen Code, statt einen ungesehenen zu benutzen. Die Passphrase (K5) wird zweimal eingegeben; zu kurze oder abweichende Eingaben lehnt schon das Formular ab, die Warnung steht daneben. Der Import wählt die Datei im Systemdialog, fragt nach Code oder Passphrase (Tippfehler und „falsch oder beschädigt" werden getrennt gemeldet, nach I1 ohne Hinweis auf die Ursache bei Letzterem), zeigt bei vorhandenen Daten die Zählung beider Seiten samt offenen Sitzungen und ändert erst nach „Daten ersetzen" etwas; Abbrechen ändert nichts. Danach zeigt die Zusammenfassung die Zahlen, den Pfad der Sicherung und den Namen des Ordners „Previous data …"; mit „Fertig" laden sich alle Ansichten neu. Fehlt dem Gerät beim Bestätigen der Backup-Schlüssel, läuft zuerst K2. Die Sicherung vor dem Import geht in das Standardverzeichnis „Smart SSH backups" im Dokumentenordner; eine Auswahl des Zielorts bietet die Oberfläche noch nicht. Der ssh_config-Export zeigt den Hinweis aus T2; der ssh_config-Import lehnt eine Backup-Datei mit Verweis auf „Daten und Backup" ab (T3), der Backup-Import eine fremde Datei als „keine Smart-SSH-Backup-Datei". Die Testfälle 14 (Oberfläche), 17 und 18 sind automatisiert.
- Umsetzungsstand Änderungssatz (#670): Die Anwendungslogik bietet für Integrationen zwei öffentliche Bausteine auf denselben typisierten Strukturen wie der Export: einen konsistenten Lesezugriff auf den ganzen Bestand (Inhalt wie im Export, C1/C2) und das Anwenden eines Änderungssatzes (je Eintrag „anlegen oder ersetzen“ bzw. „löschen“, für jeden Datentyp) in **einer** Transaktion. Das Ergebnis muss dieselbe Prüfung bestehen wie eine importierte Datei (U5 Verweise und Zyklen, U6 Regeln, F12 Grenzen); scheitert irgendetwas, auch mitten im Schreiben, ändert sich nichts. Das Löschen eines Servers entfernt wie in der Oberfläche seine Tags, Notizen samt Revisionen, Zugangsdaten und serverbezogenen Regeln (sein Chat-Verlauf folgt über das Schema); jeder andere Eintrag, der danach ins Leere zeigt, lässt den ganzen Satz scheitern. Host-Key-Änderungen nutzen den Vermerk „ausstehend“ des Imports (I5). Nach dem Abschluss laden die Ansichten neu. Die Markierung „Kopie“ eines Servers oder einer Gruppe (C1) gehört zur Zeile: ein Änderungssatz setzt sie beim Anlegen und überschreibt sie beim Ersetzen mit dem Wert der Zeile; ohne Angabe gilt „keine Kopie“. Sie hat weiterhin keine Wirkung auf Verbindung, Regeln oder Filter. Es gibt keinen neuen Tauri-Befehl.
