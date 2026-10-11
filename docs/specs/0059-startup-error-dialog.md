# Spec 0059 — Fataler Startfehler zeigt immer eine Meldung

Status: umgesetzt
Zweck: Kann Smart SSH nicht starten, sieht der Nutzer eine verständliche Meldung mit Ursache, Datenpfad und nächstem Schritt — kein Aufblitzen ohne Fenster, kein stiller Absturz.
Bezüge: Spec 0071 (Schlüsselbund nicht verfügbar, Sprache der Startdialoge), Spec 0101 (verschlüsselte Datenbank, Dialoge D1–D4, Entsperrmaske), Spec 0094 (Logregeln), ADR 0052 (Mechanismus und offene Entscheidungen), ADR 0093.

## 1. Grundsatz

Tritt vor dem ersten Fenster ein Fehler auf, der den Start unmöglich macht,
zeigt Smart SSH einen **nativen Dialog des Betriebssystems** (es gibt zu
diesem Zeitpunkt noch kein App-Fenster). Der Dialog blockiert, nennt die
Ursache, den Datenpfad und einen nächsten Schritt; danach beendet sich die
App mit Fehlerstatus. Sie läuft nie in einem halb aufgebauten Zustand
weiter.

- **Logging zuerst.** Der Fehler steht im Log, bevor der Dialog erscheint;
  der Dialog ersetzt das Log nicht. Auch ein Panic wird mit seiner Ursache
  geloggt.
- **Sprache.** Titel und Text folgen der Sprache der Systemumgebung, mit
  Deutsch als Vorgabe (Spec 0071, A11a/A11b). Die englischen Fassungen sagen
  dasselbe wie die deutschen.
- **Anzeige.** Der Datenpfad wird bereinigt angezeigt: Steuer- und
  Richtungszeichen erscheinen als `?`, damit ein Pfad den Text nicht optisch
  fortsetzen kann.
- **Dialoge mit Wahl.** Ein Dialog mit Wahl (Spec 0101, D1–D4:
  „Erneut versuchen", „Beenden", ggf. „Master-Passwort einrichten") gehört
  zu Spec 0101. Wählt der Nutzer dort „Beenden", endet die App ohne
  zweiten Dialog und mit Status 0.

## 2. Die Fehlerfälle

Die Fälle 1 bis 4 sind die ursprünglich geforderten; die Nummern bleiben
stabil, auch wo ein Fall inzwischen von einer anderen Spec getragen wird.

1. **Datenbank von einer neueren Version angelegt** (älteres Programm trifft
   neuere Datenbank). Die Meldung nennt beide Versionsnummern („Datenbank-
   Version X, dieses Programm kennt Versionen bis Y"), rät, die neueste
   Version zu installieren, und nennt den Datenpfad.
2. **Datenbank nicht lesbar oder beschädigt.** Die Meldung ist bewusst
   zurückhaltend („konnte nicht geöffnet werden — möglicherweise beschädigt,
   oder von einem anderen laufenden Programm gesperrt"), weil dieser Fall
   auch jeden anderen nicht eigens benannten Fehler beim Öffnen oder
   Migrieren abdeckt. Nächster Schritt: prüfen, ob eine zweite Instanz läuft;
   Details stehen im Log, dessen Ordner genannt wird; hilft das nicht, ein
   Backup einspielen oder den Support kontaktieren. Dazu der Datenpfad.
3. **Schlüsselbund beim Start nicht verfügbar** (gesperrt, kein Anbieter,
   kein Session-Bus). Das ist kein Panic mehr und läuft nicht mehr in eine
   Warnung mit weiterlaufender App: Ohne den Schlüssel im Schlüsselbund lässt
   sich die verschlüsselte Datenbank nicht öffnen, der Start endet im
   Dialog D1 aus Spec 0101. Ursache, blockierte Funktionen und der eine
   nächste Schritt (unter Linux mit Paketnamen, sonst OS-neutral) stehen in
   Spec 0071.
4. **Datenverzeichnis nicht lesbar oder nicht beschreibbar.** Die App prüft
   das aktiv, indem sie im Datenverzeichnis kurz eine leere Testdatei anlegt
   und wieder löscht; auch ein bestehendes, aber nur nicht mehr
   beschreibbares Verzeichnis wird so erkannt. Eine nur nicht beschreibbare
   Datenbankdatei wird ebenfalls erkannt, ohne sie zu verändern. Die Meldung
   nennt „Zugriff verweigert", rät, die Zugriffsrechte zu prüfen, und nennt
   den Datenpfad. Dieser Fall führt nie zum Rat „Backup einspielen" — die
   Datenbank ist intakt.

Weitere Startfehler folgen demselben Muster (Ursache, Pfad, nächster
Schritt, ohne Backup-Rat, wo ein Backup nichts hülfe):

- **Host-Key-Speicher nicht ladbar** (Datei beschädigt oder Zugriffsproblem;
  genannt wird der Pfad des Host-Key-Speichers, nicht der der Datenbank).
  Die Meldung warnt, dass eine nach dem Löschen erneut bestätigte
  Server-Kennung **keinen** Schutz vor einem seither untergeschobenen Server
  bietet und gegen eine vertrauenswürdige Quelle zu prüfen ist. Diese
  Warnung steht in beiden Sprachen gleich deutlich.
- **Schlüssel passt nicht zur Datenbank** (Datei beschädigt oder zu einem
  anderen Schlüssel gehörig): Ohne den Schlüssel ist auch ein Backup nicht
  lesbar; die Meldung sagt das.
- **Umwandlung, Umzug der gespeicherten Geheimnisse oder Umstellung der
  Chat-Inhalte gescheitert:** Die Meldung sagt, ob nichts verändert wurde und
  dass die Datenbank selbst offen und nicht beschädigt ist; der nächste
  Schritt ist, Platz und Schreibrechte zu prüfen und neu zu starten.
- **Datenbankdatei ist ein Symlink:** wird nicht automatisch verschlüsselt;
  nichts wird verändert; die Meldung nennt den Schritt.
- **Master-Passwort einrichten oder „Neu anfangen" gescheitert:** Die Meldung
  sagt genau, ob nichts verändert wurde oder wo die zur Seite gelegten Dateien
  liegen, und rät nie zu einem Backup.
- **Smart SSH läuft bereits mit diesem Datenverzeichnis:** Die Meldung nennt,
  zur laufenden Instanz zu wechseln oder sie zu beenden; es wurde nichts
  verändert. (Wo die laufende Instanz sich selbst nach vorn holen kann,
  erscheint kein Dialog.)

Läuft die App im Master-Passwort-Modus (Spec 0101), erscheinen
Startfehler nach der Entsperrung als Meldung in der Entsperrmaske statt als
nativer Dialog, damit das Fenster nicht verschwindet, bevor der Nutzer den
Grund gelesen hat.

## 3. Sicherheitszusagen

- **Kein Inhalt im Dialog.** Der Text nennt nur Fehlerart, Datenpfad und
  nächsten Schritt. Er enthält weder Datenbankinhalte noch Schlüssel,
  Passwörter oder rohe Fehlertexte einer Bibliothek; Letztere stehen nur im
  Log.
- **Nur melden, nie reparieren.** Der Dialog ändert nichts an den Daten.
  Reparatur ist Sache des Nutzers oder des Supports.
- **Kein Absturz ohne Meldung** in den genannten Fällen.
- **Kein Rat zur Abschwächung.** Kein Text empfiehlt einen weniger sicheren
  Speicherort für Geheimnisse.

## 4. Grenzen

- Ein Fehler, der im Log-Verzeichnis selbst auftritt (bevor das Logging
  steht), kann nicht geloggt werden.
- Die visuelle Darstellung des nativen Dialogs unterscheidet sich je
  Plattform; verbindlich ist der Inhalt, nicht das Aussehen.
- Fehler, die erst nach dem Start des Fensters auftreten, behandelt Spec 0098.
