### Neu
- Öffnest du die KI-Provider-Einstellungen ohne konfigurierten
  Ollama-Provider, sucht die App einmalig ein lokal laufendes Ollama
  (`127.0.0.1:11434`) und bietet an, es zu übernehmen — inklusive
  Modellauswahl. Ohne gefundenes Ollama zeigt eine kompakte Anleitung, wie
  man es installiert. Kein Netzwerkaufruf ohne diese Aktion oder einen
  Klick auf „Erneut suchen".
- Ist noch kein eigener Server angelegt, führt die Serverliste jetzt aktiv
  mit „Ersten Server anlegen" in den Anlage-Dialog, statt nur einen
  Entwicklertext zu zeigen.
- Beim Anlegen eines Servers mit Schlüssel-Anmeldung empfiehlt ein Hinweis
  Ed25519-Schlüssel (`ssh-keygen -t ed25519`) — RSA bleibt unterstützt.
- Bei Ollama ist der API-Key im Formular nicht mehr erforderlich.

### Geändert
- Fehlermeldungen auf dem Weg „Provider einrichten → Server anlegen →
  Verbindung testen → verbinden" nennen jetzt durchgängig Ursache **und**
  nächsten Schritt, auf Deutsch und Englisch — u. a. für falschen API-Key,
  unbekanntes Modell, nicht erreichbaren lokalen KI-Dienst, abgelehnte
  SSH-Verbindung, unbekannten Host, nicht erreichbaren Host und
  abgelehnten/nicht rechtzeitig bestätigten Host-Key.
- Der SSH-Verbindungsaufbau bricht jetzt nach 10 Sekunden ohne Antwort mit
  einer klaren Meldung ab, statt minutenlang auf das Betriebssystem zu
  warten.
