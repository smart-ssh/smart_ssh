# Spec 0040 — Sitzungs-Persistenz: Zusagen und Korrekturen

Status: umgesetzt
Zweck: Ergänzende Regeln zur Chat-Persistenz (Spec 0034): Nutzer-Nachrichten
werden gespeichert, MCP-Inhalte bleiben draußen, Redaction läuft vor jedem
Senden erneut, und ein paar Randfälle verhalten sich sauber.
Bezüge: Spec 0034 (Persistenz), Spec 0036 und 0101 (Verschlüsselung), Spec
0039 (Fencing/Eskalation), Spec 0028/0104 (MCP), Spec 0003/0010
(Notiz-Vorschlag), Spec 0015 (Prompt-Historie), Spec 0057 (Session-Modell).

## 1. Zusammenhang

Nutzer-Nachrichten, MCP-Herkunft und Redaction beim Senden hängen am selben
Pfad (Verlauf schreiben, Kontext für die Anfrage vorbereiten) und werden
darum zusammen beschrieben. Der gravierendste Zusammenhang: Würde der
Nutzertext nicht im (geschützten) Chat-Verlauf landen, läge die einzige
Kopie ungeschützt in der Eingabe-Historie.

## 2. Nutzer-Nachrichten werden persistiert

Jede Nutzer-Nachricht wird vor dem KI-Aufruf auf demselben Weg wie alle
anderen Verlaufseinträge gespeichert (redigiert, wo zutreffend), wie in
Spec 0034, Abschnitt 4 als eine der fortlaufend zu speichernden
Nachrichtenarten vorgeschrieben. Der Test dafür setzt am Senden einer
Chat-Nachricht an, nicht erst tiefer in der Schleife.

## 3. Eingabe-Historie

Die Eingabe-Historie (Spec 0015) enthält freien, womöglich sensiblen
Nutzertext. Sie ist wie der Chat durch die Verschlüsselung der gesamten
Datenbankdatei geschützt (Spec 0036, Spec 0101); es gibt keine zweite,
feldweise Verschlüsselung.

## 4. MCP-Herkunft bleibt außerhalb der gespeicherten Historie

MCP-ausgelöste Aktionen erzeugen **keine** Chat-Sitzung und schreiben
**nichts** in die gespeicherte, fortsetzbare Historie (Spec 0034,
Abschnitt 10; Spec 0028, Spec 0104).

- Fällt eine MCP-Aktion in einen bereits offenen Tab des Nutzers, erscheint
  sie im Live-UI (Bestätigungsdialog, Ergebnis), wird aber **nicht** in
  dessen gespeicherten Verlauf geschrieben. Der fortsetzbare Cache bleibt
  frei von MCP-Inhalten.
- Das gilt auch dann, wenn ein Tab des Nutzers für denselben Server offen
  ist.
- Die Markierung „nicht vertrauenswürdiger Inhalt gesehen" (Spec 0039)
  greift unabhängig davon; es gibt keine Umgehung der Filter-Engine.

## 5. Redaction läuft beim Senden erneut — nur additiv

Vor jeder Anfrage an die KI läuft ein Redaction-Durchlauf über den
gesamten Verlauf, der gesendet wird. Anlass: Ein Secret kann persistiert
worden sein, bevor das passende Muster bekannt war (z. B. ein erst später
hinterlegtes Sudo-Passwort); ohne den erneuten Durchlauf ginge es bei einer
Fortsetzung unverändert an den Anbieter.

Der Durchlauf ist **nur additiv**: Er darf zusätzlich redigieren, aber nie
bereits vorhandene Redaction entfernen und nie Inhalt sichtbar machen, der
vorher nicht gesehen wurde. Neue Muster greifen, es kommt nie *mehr* Inhalt
heraus als beim ersten Senden.

## 6. „In Notiz übernehmen"

An einer Chat- bzw. Ergebnis-Zeile gibt es eine Aktion „in Notiz
übernehmen". Sie füllt den bestehenden Notiz-Vorschlag-Ablauf (Spec 0003,
Abschnitt 5.2) mit dem Inhalt der Zeile vor, inklusive dessen
Bestätigungsdialog. Es gibt keinen eigenen Speicherweg: Chat-Cache, Notizen
und Audit-Log bleiben getrennt.

## 7. Randfälle

1. **Gesperrter Schlüsselbund.** Ein Fehler beim Zugriff auf einen
   gesperrten oder verweigerten Schlüsselbund bricht nicht die ganze App
   ab; betroffen ist nur die Chat-Persistenz (klare Fehlermeldung,
   Chat-Cache deaktiviert).
2. **Fehlgeschlagenes Fortsetzen.** Schlägt das Laden einer Sitzung nach
   dem Verbindungsaufbau fehl, wird die Verbindung sauber getrennt.
3. **Aktive Sitzung löschen.** Löschen einer gerade aktiven Sitzung wird
   erkannt und mit klarer Meldung verhindert; die Live-Sitzung schreibt
   nie in eine gelöschte Sitzung.

## 8. Bewusste Vereinfachungen

- Das Kontext-Budget für gesendeten Verlauf ist fest und kein Bedienknopf.
- Der Chat des lokalen Pseudo-Servers wird nicht gespeichert (kein
  Server-Eintrag, an den die Sitzung gebunden wäre).
- Eine KI-generierte Datei wird als normale Assistenten-Text-Nachricht
  gespeichert, nicht als eigene Inhaltsart.

## 9. Nicht Teil dieser Spec

- Das reiche Session-Modell (Ledger, Zusammenfassung, Kompaktierung)
  beschreibt Spec 0057.

## 10. Sicherheitszusagen

- Nutzer-Nachrichten landen im geschützten Chat-Verlauf, die Eingabe-
  Historie ist durch dieselbe Datenbank-Verschlüsselung geschützt.
- MCP-Inhalte landen nie in einer gespeicherten, fortsetzbaren Historie.
- Der Redaction-Durchlauf beim Senden ist nur additiv.

## 11. Akzeptanzfälle

- Nutzertext ist nach dem Senden einer Chat-Nachricht gespeichert.
- Eine MCP-Aktion erzeugt, auch bei offenem Tab des Nutzers, weder
  Sitzung noch Nachrichtenzeile.
- Ein nachträglich hinzugekommenes Muster greift beim erneuten Senden;
  nichts zuvor Redigiertes wird sichtbar.
- Gesperrter Schlüsselbund lässt die App starten (Chat-Cache deaktiviert);
  fehlgeschlagenes Fortsetzen trennt sauber; Löschen einer aktiven
  Sitzung erzeugt keine verwaiste Schreibung.
