# ADR 0121 — Single-Instance-Schutz nur mit dem Standard-Datenverzeichnis

Status: akzeptiert
Betrifft: Issue #44, ergänzt ADR 0106 §5/§6 und löst dort eine Konsequenz
ab; Spec 0101 §1; ADR 0032

## Kontext

`tauri-plugin-single-instance` erkennt Instanzen nur an der App-Kennung
(`identifier`), nicht am Datenverzeichnis. Eine eigene Instanz-Kennung lässt
die Version 2.4.5 nicht zu. Mit ADR 0106 führte das in Release-Builds zu
zwei Problemen:

1. Zwei Release-Instanzen mit verschiedenen Datenverzeichnissen
   (`SMART_SSH_DATA_DIR`) konnten nicht nebeneinander laufen. Der zweite
   Start holte die erste Instanz nach vorn und endete.
2. Davor hatte der zweite Prozess die Sperre auf **sein** Verzeichnis
   bekommen und im Schlüsselbund-Modus dessen Datenbank schon geöffnet,
   migriert und womöglich umgewandelt (Spec 0101, A6). Das Plugin prüft
   erst in `Builder::build`, also nach diesem Zugriff.

Issue #44 nennt drei Wege: A) das Plugin nur mit dem Standard-
Datenverzeichnis, B) eigene IPC mit einem Schlüssel aus dem kanonischen
Datenverzeichnis, C) die Plugin-Prüfung vor das Öffnen der Datenbank ziehen.
Ohne anderslautende Entscheidung gilt A.

## Entscheidung

**Am Single-Instance-Schutz nimmt nur ein Prozess mit dem
Standard-Datenverzeichnis teil.** Ist `SMART_SSH_DATA_DIR` gesetzt und nicht
leer, bleibt das Plugin in diesem Prozess aus: Es wird weder am Builder
registriert noch im Pfad „Sperre belegt" (ADR 0106 §5) gebaut. Ein leerer
Wert gilt wie bei der Auflösung des Datenverzeichnisses als nicht gesetzt.
Diese Regel steht an einer Stelle (`persistence_sqlite::data_dir_override_from`)
und wird von beiden Seiten benutzt.

Die Entscheidung selbst ist eine reine Funktion ohne Tauri
(`app_logic::single_instance::focusing_applies`, Eingaben: Rohwert der
Variablen und Build-Art). `app-shell` liest nur Umgebung und `cfg!` und
registriert das Plugin.

Warum das Anforderung 3 aus Issue #44 vollständig erfüllt: Alle Teilnehmer
benutzen dasselbe Datenverzeichnis, nämlich das Standardverzeichnis dieses
Builds. Ein zweiter Teilnehmer findet die Sperre deshalb immer belegt und
geht in den Pfad ohne Datenbankzugriff. Den Fall „Sperre frei, Datenbank
geöffnet, dann vom Plugin beendet" gibt es damit nicht mehr. Ein Prozess mit
Override nimmt nicht teil und kann von keinem anderen beendet werden.

Die Sperre auf das Datenverzeichnis (ADR 0106 §1–§4) bleibt unverändert, und
jeder Fehler beim Sperren beendet den Start weiterhin.

**Debug-Builds** behalten ihr Verhalten (Plugin aus, Sperre an). Die
Einschränkung auf Release-Builds bleibt nötig: Debug- und Release-Build
tragen dieselbe Kennung und haben verschiedene **Standard**-Verzeichnisse
(ADR 0032). Ohne Override würden sich beide sonst gegenseitig beenden.

## Konsequenzen

- Release-Build, Standardverzeichnis, zweiter Start: wie bisher, die
  laufende Instanz kommt nach vorn, der zweite Prozess endet mit 0, die
  Datenbank bleibt unberührt.
- Release-Build mit Override: Instanzen mit verschiedenen Verzeichnissen
  (auch eine mit dem Standardverzeichnis daneben) laufen nebeneinander. Ein
  zweiter Start mit **demselben** Override-Verzeichnis holt nichts nach
  vorn, sondern zeigt den Startfehler „Smart SSH is already running with
  this data directory". Die Datenbank bleibt unberührt.
- Zeigt `SMART_SSH_DATA_DIR` ausdrücklich auf das Standardverzeichnis, gilt
  der Prozess als Override-Prozess. Trifft er auf eine laufende
  Standard-Instanz, endet er mit dem Startfehler statt sie nach vorn zu
  holen. Umgekehrt findet eine Standard-Instanz die Sperre belegt, kein
  Teilnehmer antwortet, und sie zeigt ebenfalls den Startfehler. Beides ist
  datenseitig unkritisch.
- Keine neue Abhängigkeit, keine eigene IPC, kein neues Datenformat. Option B
  bleibt möglich, falls Fokussieren auch mit Override gewünscht wird.
- Linux ohne D-Bus-Sitzung: unverändert, das Plugin meldet sich dort nicht
  an, die Sperre schützt die Daten.
