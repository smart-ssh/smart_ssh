### Sicherheit
- Die KI-Zweitmeinung zum Datenrisiko und die Prüfung auf eingeschleuste
  Anweisungen lesen ihr Urteil jetzt zuverlässig aus der Antwort: Enthält
  eine Antwort mehrere Urteilswörter, zählt das warnende. Bisher zählte das
  zuerst genannte — eine Antwort, die den geprüften Text zitierte und darin
  ein „nein"/„none" enthielt, konnte so eine danach ausgesprochene Warnung
  verschlucken. Bei der Prüfung auf eingeschleuste Anweisungen ließ sich das
  gezielt ausnutzen, weil der geprüfte Text dort aus einer nicht
  vertrauenswürdigen Quelle stammt. Eine Warnung kann jetzt nicht mehr durch
  die Wortstellung verlorengehen.

### Geändert
- Als Folge davon kann die Prüfung auf eingeschleuste Anweisungen jetzt
  häufiger anschlagen — etwa wenn das Modell in seiner Begründung eine
  gewöhnliche Konfigurationszeile wie `PermitRootLogin yes` zitiert. Die
  nächste Aktion verlangt dann eine Bestätigung, statt automatisch zu
  laufen. Das ist die sichere Richtung: Eine Nachfrage zu viel ist sichtbar
  und mit einem Klick erledigt, eine verschluckte Warnung nicht.
