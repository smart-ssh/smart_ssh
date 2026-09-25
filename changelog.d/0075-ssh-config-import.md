### Neu
- Server-Profile lassen sich aus einer OpenSSH-`ssh_config` einlesen. Eine
  Vorschau zeigt vorher vollständig, was entstehen würde — Felder samt
  Herkunft, die gelesenen Dateien mit vollem Pfad, die entstehende
  Ordnerstruktur, Namenskonflikte und jede Zeile, die **nicht** übernommen
  wird. Einzelne Einträge lassen sich abwählen; wer abbricht, legt nichts
  an. Ein zweiter Import derselben Datei erzeugt nichts doppelt.
- `Include` wird mitgelesen, und jede eingebundene Datei wird ein
  Unterordner. `ProxyJump` wird auf die Jump-Host-Verkettung abgebildet;
  eine Kette, die sich im Kreis dreht, wird beim Import abgelehnt statt
  erst beim Verbinden aufzufallen.
- Bei einer `IdentityFile`-Zeile entscheidet der Nutzer, was geschehen
  soll: den Pfad als Schlüsseldatei übernehmen (Vorgabe — die Datei wird
  dabei nicht geöffnet), den Schlüssel einlesen und im Schlüsselbund
  ablegen, oder die Zeile weglassen. Zeigt ein Pfad ins Leere, fällt nur
  dieser eine Eintrag zurück; der Import läuft weiter und sagt, warum.

### Sicherheit
- Der Import öffnet von sich aus ausschließlich `ssh_config`-Dateien. Eine
  Schlüsseldatei wird nur gelesen, wenn man das ausdrücklich verlangt, erst
  beim Bestätigen, und nur die Dateien, die die Vorschau vorher namentlich
  genannt hat.
- Aus einer eingelesenen Datei gelangt kein Inhalt auf den Bildschirm: Eine
  eingebundene Datei, die keine `ssh_config` ist, wird mit ihrem Pfad
  gemeldet und übersprungen; von einer nicht übernommenen Zeile erscheint
  nur der Name der Direktive mit Zeilennummer, nie ihr Wert.
- Eine importierte Datei kann keine Sicherheitseinstellung eines Profils
  setzen — Eskalation nach Serverinhalt und KI-Prüfung stehen immer auf den
  Vorgaben des Produkts.
- Übermäßig große, tief verschachtelte oder sich selbst einbindende
  Konfigurationen führen zu einer verständlichen Meldung statt zu einem
  hängenden oder abstürzenden Programm.
