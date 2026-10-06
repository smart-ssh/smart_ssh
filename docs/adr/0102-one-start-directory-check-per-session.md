# ADR 0102 — Eine Startverzeichnis-Prüfung und ein Hinweis pro Sitzung

Status: akzeptiert
Betrifft: Spec 0102 (§4), Issue #9

## Problem

Terminal und Dateibrowser öffnen unabhängig voneinander und in beliebiger
Reihenfolge. Beide sollen dasselbe Ergebnis sehen (Verzeichnis da oder
nicht), und ein fehlendes Verzeichnis soll genau **einen** Hinweis pro
Sitzung erzeugen, nicht zwei.

## Entscheidung

1. **Die Prüfung liegt an der Sitzung**, nicht am Terminal oder am Browser:
   `Session` trägt den konfigurierten Wert (gesetzt beim Bau über
   `with_start_directory`) und das Ergebnis in einem `tokio::sync::OnceCell`.
   Wer zuerst fragt, löst die einzige SFTP-`stat`-Prüfung aus; parallele
   Aufrufer warten auf dieselbe.
2. **Der Hinweis wird einmal herausgegeben:** Ein `AtomicBool` an der
   Sitzung sorgt dafür, dass der konfigurierte Wert nur im ersten Ergebnis
   von `open_terminal` bzw. `sftp_start_directory` steht. Das Frontend zeigt
   ihn ohne eigene Buchführung als Toast. Wird ein Terminal oder Browser neu
   geöffnet, kommt kein zweiter Hinweis.
3. **SFTP nicht verfügbar zählt als „fehlt"**: Ohne SFTP lässt sich die
   Existenz nicht prüfen; Terminal und Browser bleiben im Home, und der
   Hinweis erscheint. Ein `cd` ohne Prüfung hätte die gemeinsame Aussage
   beider Ansichten aufgegeben.
4. **Ein Ziel, das kein Verzeichnis ist**, zählt ebenfalls als „fehlt".
5. **Der Wert wird nicht in `SessionParts` aufgenommen**, sondern als
   privates Feld an `Session` (wie der normale SFTP-Kanal, Spec 0085). So
   erreicht ihn der KI-Ausführungspfad nicht über die öffentlichen Felder,
   und kein bestehender Sitzungs-Konstruktor musste geändert werden.

## Abgewogene Alternative

Ein Tauri-Event `start-directory-missing`, auf das ein globaler Listener
reagiert: hätte eine zusätzliche Abhängigkeit auf einen `EventEmitter` in
die Prüfung gebracht und ein Event vor dem Registrieren des Listeners
verlieren können. Der Rückgabewert der beiden Commands ist einfacher und
direkt testbar.

## Konsequenzen

- Ein später angelegtes Verzeichnis wird erst in der nächsten Sitzung
  erkannt (die Prüfung ist pro Sitzung gecacht).
- Ein Verzeichnis, das nach der Prüfung verschwindet, lässt `cd` bzw. das
  Listing sichtbar scheitern — dieselbe Fehleranzeige wie bei jeder anderen
  Navigation.
