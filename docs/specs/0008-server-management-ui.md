# Spec 0008 — Server- und Gruppenverwaltung

Status: umgesetzt
Zweck: Server und Gruppen lassen sich vollständig in der Oberfläche anlegen, bearbeiten und löschen. Notizen haben einen Editor mit Änderungshistorie, und der Nutzer kann sehen, was die KI als Kontext zu sehen bekommt. Ein Verbindungstest prüft Zugangsdaten, bevor ein Server gespeichert wird.
Bezüge: Spec 0003 (Profile, Notizen), Spec 0004 (Persistenz, Löschverhalten), Spec 0005 (Verbindung, Host-Key-Prüfung, Zeitgrenzen, Schritt-Protokoll), Spec 0007 (Hauptansicht, Umgang mit Zugangsdaten), Spec 0033 (gruppierte Serverübersicht), Spec 0046 (zweistufiges Löschen eines Servers), Spec 0069 (Fehlerarten), Spec 0073 (Trimmen von Zugangsdaten), Spec 0075 (SSH-Konfiguration), Spec 0082 (Wechsel der Anmeldeart), Spec 0100 (Host-Key-Dialog), Spec 0101 (Secret-Umzug), Spec 0103 (Drag-and-drop), ADR 0110 (Schritt-Protokoll des Verbindungstests).

## 1. Ziel

Server und Gruppen werden in der Oberfläche verwaltet statt über Demo-Programme
oder die Kommandozeile. Notizen (Spec 0003, Abschnitt 5) haben einen echten
Editor samt Änderungshistorie, und der Nutzer kann sich anzeigen lassen, was
die KI als Kontext tatsächlich sieht.

## 2. Abgrenzung

Nicht Teil dieser Spec:

- Drag-and-drop-Umordnen von Servern und Gruppen: Spec 0103. Die Auswahl der
  übergeordneten Gruppe per Auswahlliste im Bearbeiten-Formular bleibt als
  tastaturbedienbarer Weg bestehen.
- Mehrere parallele Sitzungen: Spec 0017.
- Das Verwalten der Filter-Regeln: Spec 0009. Tags lassen sich am Server als
  Freitext-Chips setzen; die Zuordnung zu Regeln ist dort beschrieben.

## 3. Gruppen

Die Oberfläche kann Gruppen auflisten (flach, mit Verweis auf die übergeordnete
Gruppe; den Baum baut die Oberfläche), anlegen, umbenennen oder verschieben und
löschen.

Das Löschen einer Gruppe ist zweistufig, passend zum Löschverhalten aus Spec
0004: Der erste Schritt löscht nichts und liefert eine Vorschau,

- welche Untergruppen mitgelöscht werden (kaskadierend) und
- welche Server nur ihre Gruppenzuordnung verlieren.

Erst ein zweiter, ausdrücklich bestätigter Schritt löscht. So werden nicht
versehentlich mehrere Untergruppen gelöscht, ohne sie vorher gesehen zu haben.

## 4. Server

Die Oberfläche kann Server auflisten (optional nach Gruppe), einzeln abrufen,
anlegen, ändern, löschen und eine Verbindung testen (Abschnitt 7).

Ein Server hat: Name, Host, Port, Benutzername, optionale Gruppe, Tags, eine
Anmeldeart und optional einen Jump-Host (einen anderen gespeicherten Server).
Anmeldearten sind Passwort, privater Schlüssel (Inhalt mit optionaler
Passphrase), Agent und Zertifikat (Zertifikat plus Schlüssel); weitere Arten
beschreibt Spec 0076.

**Umgang mit Zugangsdaten.** Wie bei der KI-Anbieter-Verwaltung (Spec 0007,
Abschnitt 8.2):

- Geheime Felder sind nur beim Anlegen zwingend. Beim Ändern heißt ein leeres
  Feld „unverändert lassen", nicht „löschen" (Spec 0073, Spec 0082).
- Das Backend legt Secret-Inhalte in der Credential-Ablage ab, **bevor** die
  übrigen Felder in der Datenbank stehen. So gibt es nie einen Verweis auf ein
  Secret, das nicht existiert.
- Was die Oberfläche zurückbekommt, enthält **keine** Secret-Felder, nur welche
  Anmeldeart aktiv ist.

**Aufbau des Server-Formulars.** Die Einstellungen eines Servers sind auf
vier Reiter verteilt: **Allgemein**, **Verbindung**, **Sicherheit** und
**Notizen**. Felder und Aktionen ändern sich dadurch nicht, nur ihr Ort:

- *Allgemein*: Name, Gruppe, Tags, danach die Abschnitte von Erweiterungen und
  am Ende „Server löschen" samt Auswirkungs-Vorschau.
- *Verbindung*: Host, Port, Benutzername, Jump-Host, Anmeldung (alle Arten,
  Schlüsselprüfung, Übernahme in den Schlüsselbund), Sudo-Passwort,
  Startverzeichnis, „Erweitert" (SFTP-Server-Pfad) sowie „Verbindung testen"
  mit Ergebnis und Schritt-Protokoll.
- *Sicherheit*: Bestätigung nach Serverzugriff (Spec 0039), KI-Prüfung auf
  Prompt-Injection und die gespeicherten Host-Keys.
- *Notizen*: Notizen und Kontext-Vorschau. Die Notizen haben wie bisher ihren
  eigenen Speichern-Knopf.

Ein vorhandener Server öffnet auf *Allgemein*; solange derselbe Server gewählt
bleibt, bleibt auch der gewählte Reiter, bei einem anderen Server öffnet wieder
*Allgemein*. Alle Reiter teilen sich einen Entwurf: Ein Reiterwechsel verwirft
keine Eingaben, und ein einziger „Speichern"-Knopf (sichtbar in *Allgemein*,
*Verbindung* und *Sicherheit*) speichert alle Felder auf einmal. Ist ein Feld
ungültig (z. B. leerer Host), wird der betroffene Reiter markiert und
„Speichern" wechselt zum ersten markierten Reiter. Beim Anlegen eines neuen
Servers gibt es *Allgemein*, *Verbindung* und *Sicherheit*; *Notizen* kommt
nach dem ersten Speichern hinzu. Der lokale Pseudo-Server (Spec 0032) hat nur
*Allgemein* (Tags) und *Notizen*. Die Reiter sind per Tastatur bedienbar
(Pfeiltasten, Pos1/Ende; ein Tab-Stopp für die Leiste).

**Löschen eines Servers** ist zweistufig (Spec 0046): erst eine Vorschau der
Auswirkungen, dann das bestätigte Löschen.

## 5. Notiz-Verwaltung

- Jede manuelle Änderung der Notiz einer Gruppe oder eines Servers landet als
  Revision mit dem Nutzer als Bearbeiter in der Historie (Spec 0003,
  Abschnitt 5.3), genau wie Änderungen der KI.
- Die **Historie** ist eine chronologische Liste. Jeder Eintrag zeigt den
  Zeitpunkt und den Bearbeiter (der Nutzer, oder die KI mit Anbieter und
  Modellname) und hat eine Schaltfläche „Wiederherstellen".
- **Wiederherstellen** überschreibt nichts still: Es entsteht eine neue
  Revision mit dem alten Inhalt. Die Historie bleibt nur anfügend, vergangene
  Einträge werden weder gelöscht noch umgeschrieben, und ein Rollback bleibt
  nachvollziehbar.
- Die **Kontext-Vorschau** zeigt den wirksamen Notiztext eines Servers
  (Spec 0003, Abschnitt 5.1) unverändert: genau den Text, den die KI beim
  nächsten Verbindungsaufbau zu diesem Server als Kontext bekommt, nicht
  gekürzt und nicht umformatiert.

## 6. Oberfläche

- **Seitenleiste:** Baum der Gruppen mit den Servern als Blättern. Ein Klick auf
  eine Gruppe oder einen Server öffnet das Bearbeiten-Formular im Hauptbereich,
  nicht in einem eigenen Dialog, damit sich beim Einrichten mehrerer Server
  bequem hin- und herwechseln lässt.
- **Gruppen-Formular:** Name, übergeordnete Gruppe (Auswahlliste, die die
  Gruppe selbst und ihre Nachfahren ausschließt, sodass ein Zyklus schon vor
  dem Absenden unmöglich ist), Notiz-Editor und eine Löschen-Schaltfläche mit
  der Vorschau aus Abschnitt 3.
- **Server-Formular:** alle Felder aus Abschnitt 4. Die Anmeldeart wählt der
  Nutzer aus, die Eingabefelder wechseln mit: Passwortfeld; Schlüsseldatei über
  den Datei-Dialog mit optionaler Passphrase; „Agent verwenden" ohne weitere
  Eingabe; Zertifikat und Schlüssel als zwei Dateiauswahlen. Tags werden als
  Chips eingegeben (durch Komma oder Enter getrennt). Dazu kommen der
  Notiz-Editor und die Schaltfläche „Kontext-Vorschau", die den wirksamen
  Notiztext in einem schreibgeschützten Textblock zeigt.
- **Gespeicherte Host-Keys:** Das Formular eines bestehenden, gespeicherten
  Servers zeigt einen schreibgeschützten Bereich „Host-Keys" mit einer Zeile je
  gespeichertem Schlüssel: Algorithmus (z. B. `ssh-ed25519`) und Fingerprint
  (`SHA256:…`, markier- und kopierbar), damit der Nutzer ihn mit einer zweiten
  Quelle abgleichen kann.
  - Nachgeschlagen wird mit Host und Port des **gespeicherten** Servers, nicht
    mit ungespeicherten Formularwerten.
  - Ist noch kein Schlüssel gespeichert, steht dort „Noch kein Host-Key
    gespeichert. Er wird bei der ersten Verbindung gespeichert."
  - Bei einem neuen, ungespeicherten Server und beim lokalen Pseudo-Server
    erscheint der Bereich nicht.
  - Ein Schlüssel, dessen Algorithmus sich nicht bestimmen lässt, erscheint mit
    der neutralen Bezeichnung `unknown`.
  - Das Anzeigen verändert den Vertrauensstand nicht; Löschen oder Ersetzen
    eines Schlüssels gibt es hier nicht. Der rohe Schlüssel verlässt das
    Backend nie.
  - Server mit gleichem Host und Port teilen sich die gespeicherten Schlüssel
    und zeigen dieselben Einträge.
- **Notiz-Historie:** wie in Abschnitt 5.

## 6a. Nicht nutzbare Server

Ein gespeicherter Server, dessen Anmeldeart diese Version der App nicht
lesen kann, ist **nicht nutzbar**. Das passiert, wenn eine neuere Version
eine Anmeldeart gespeichert hat, die diese Version nicht kennt (z. B. nach
dem Zurückgehen auf eine ältere Version), oder wenn die gespeicherte
Anmeldeart beschädigt ist.

- **Die Serverliste lädt trotzdem.** Alle übrigen Server erscheinen und
  funktionieren wie gewohnt; ein einzelner nicht nutzbarer Server lässt
  weder die Liste noch den Start der App scheitern.
- **Anzeige:** In beiden Bäumen (Verbinden und Verwalten) steht der Server
  an seinem Platz in der Gruppe, deutlich als „nicht nutzbar" markiert, mit
  Name, Host und Grund:
  - unbekannte Anmeldeart: „Mit einer neueren Version der App gespeichert;
    diese Version kennt die Anmeldeart nicht."
  - beschädigte Anmeldeart: „Die gespeicherte Anmeldeart ist beschädigt
    und nicht lesbar."
- **Was nicht geht:** Verbinden, Bearbeiten, Verschieben per
  Drag-and-drop, Zugriff über MCP, Export in eine SSH-Konfiguration. Die
  Anmeldeart wird nie durch eine Ersatz-Anmeldeart (etwa „Agent")
  ersetzt — ein nicht nutzbarer Server wird nie wie ein nutzbarer
  behandelt. Wege, die einen nutzbaren Server brauchen (Verbinden, MCP,
  SSH-Konfigurations-Export, Diagnose-Export), lassen ihn aus und
  scheitern nicht an ihm.
- **Löschen:** Wählt man ihn in der Verwalten-Ansicht aus, zeigt der
  Hauptbereich den Grund und nur einen Löschen-Button, zweistufig wie beim
  normalen Server (Vorschau, dann „Endgültig löschen"). Gelöscht werden der
  Server samt Chat-Verlauf, Notizen und Tags sowie jedes Secret, das diese
  Version der App unter seiner ID speichern kann — welche er tatsächlich
  hat, ist mit unlesbarer Anmeldeart nicht bekannt. Secrets, die nur eine
  neuere Version für ihn angelegt hat, kennt diese Version nicht: Sie
  bleiben im Schlüsselbund bzw. in der Datenbank zurück und werden nicht
  als zurückgeblieben gemeldet; die Vorschau weist darauf hin. Secrets
  anderer Server werden dabei nie berührt. Lässt sich ein Secret nicht entfernen, läuft das
  Löschen trotzdem durch und nennt jeden zurückgebliebenen Eintrag, wie
  beim normalen Löschen. Andere Server, die ihn als Jump-Host nutzen,
  werden in der Vorschau genannt und verlieren diesen Verweis.
- **Zählt für Bestandsabgleiche, nie als Verbindungsziel.** Ein nicht
  nutzbarer Server zählt beim Import einer SSH-Konfiguration (Spec 0075,
  3.1.8) wie jeder vorhandene Server für die Konflikterkennung: gleicher
  Name oder gleiche Kombination aus Host, Port und Benutzer ergibt einen
  Konflikt in der Vorschau, standardmäßig wird übersprungen. Er wird dabei
  nie als `ProxyJump`-Ziel aufgelöst. Die Löschvorschau einer Gruppe nennt
  ihn — auch aus einer Untergruppe — als Server, der seine
  Gruppenzuordnung verliert (als „nicht nutzbar" gekennzeichnet); Server
  außerhalb der gelöschten Gruppe und ihrer Untergruppen stehen nicht dort.
  Weder Vorschau noch Import noch das Löschen der Gruppe ändern seine
  gespeicherte Anmeldeart.
- **Gespeicherte Daten bleiben unverändert.** Anzeigen, Auflisten und der
  einmalige Umzug der Secrets in die verschlüsselte Datenbank (Spec 0101)
  ändern die gespeicherte Anmeldeart nicht. Mit der neueren Version ist der
  Server danach wieder normal nutzbar. Der Secret-Umzug übernimmt jedes
  seiner Secrets, das diese Version kennt, statt es im Schlüsselbund
  zurückzulassen. Secrets, die nur eine neuere Version für ihn angelegt
  hat, übernimmt er nicht; sie bleiben im Schlüsselbund, auch nachdem der
  Umzug als abgeschlossen gilt.
- Andere beschädigte Felder eines Servers (ID, Zeitstempel, Port) sind
  davon nicht erfasst und lassen das Laden weiterhin scheitern.

## 7. Verbindungstest („Verbindung testen")

Der Verbindungstest prüft eine Verbindung, **bevor** ein Server gespeichert
wird, direkt aus dem offenen Anlege- oder Bearbeiten-Formular.

Mögliche Ergebnisse: Erfolg, Anmeldung fehlgeschlagen, unbekannter Host-Key,
geänderter Host-Key, Netzwerkfehler (mit Fehlercode, wenn bekannt, Spec 0069)
und Zeitüberschreitung.

- **Live-Anzeige und Abbruch.** Der Test zeigt seine Schritte live in einem
  Dialog und lässt sich abbrechen; ein abgebrochener Test endet ohne Ergebnis
  und ohne Fehler (Spec 0112).
- **Nichts wird gespeichert.** Der Test nimmt die Formularwerte einschließlich
  noch nicht gespeicherter Secrets entgegen und legt weder in der Datenbank noch
  in der Credential-Ablage etwas ab. Bei einem bestehenden Server mit leerem
  Secret-Feld („unverändert lassen") nutzt der Test das bereits gespeicherte
  Secret dieses Servers, sonst ließe sich ein bestehender Server nur nach
  erneutem Eintippen des Passworts testen.
- **Nur Anmeldung, kein Kommando.** Der Test baut die Verbindung auf und meldet
  sich an. Es wird kein Kommando ausgeführt, die Filter-Engine ist nicht
  beteiligt, und auf dem Server passiert nichts. Danach wird die Verbindung
  sofort geschlossen; es entsteht keine Sitzung.
- **Zeitgrenzen** wie beim echten Verbindungsaufbau (Spec 0005, Abschnitt 7;
  Spec 0069): Statt unbegrenzt zu warten, meldet der Test „Timeout". Das
  Warten auf eine Host-Key-Entscheidung zählt nicht dazu.
- **Jump-Host-Kette:** Bereits gespeicherte Zwischen-Hops werden wie bei einer
  echten Verbindung aufgelöst; nur der letzte Hop, der gerade bearbeitete
  Server, nutzt die frischen Formulardaten.
- **Host-Key-Prüfung ist dieselbe wie bei einer echten Verbindung.** Die
  gespeicherten Schlüssel sind nach Host und Port abgelegt, nicht nach Server.
  Bei einem unbekannten Schlüssel erscheint derselbe Bestätigungsdialog (Spec
  0005, Abschnitt 6; Spec 0100) mit dem Schlüsseltyp des angebotenen Schlüssels
  (ohne diese Zeile, wenn er sich nicht bestimmen lässt). Stimmt der Nutzer zu,
  wird der Schlüssel gespeichert, und die spätere echte Verbindung fragt nicht
  erneut. Bei einem geänderten Schlüssel erscheint der Warndialog.
- **Schritt-Protokoll:** Der Test zeigt die Schritte des Aufbaus mit Dauer und
  Ergebnis zugeklappt unter der Meldung (Spec 0005, Abschnitt 7). Es enthält
  keine Zugangsdaten und geht nie ins Log oder in den Diagnose-Export.

Das Ergebnis erscheint kompakt im Formular (grüner Haken bei Erfolg, sonst eine
kurze Fehlermeldung), ohne eigenes großes Fenster.

## 8. Grenzen

- Die Auswahl einer Schlüsseldatei liest den Dateiinhalt und legt ihn in der
  Credential-Ablage ab; der ursprüngliche Dateipfad wird **nicht** gespeichert.
  Rotiert der Nutzer den Schlüssel später außerhalb der App, muss er ihn im
  Formular erneut auswählen. Die App beobachtet die Datei nicht auf Änderungen.
  (Identitätsdateien, die die App dauerhaft per Pfad liest: Spec 0076.)
