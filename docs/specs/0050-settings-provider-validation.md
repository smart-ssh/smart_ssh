# Spec 0050 — Einstellungen: Struktur und Provider-Key-Prüfung

Status: umgesetzt
Zweck: Der Einstellungsbereich ist zweispaltig gegliedert und das
Anbieter-Formular hilft beim Eingeben eines API-Keys (Format-Hinweis,
Test-Knopf).
Bezüge: Spec 0038 (Erweiterungs-Registry), Spec 0006/0007/0025
(KI-Anbieter), Spec 0008 (Verbindungstest als Vorbild), Spec 0049
(Trim der Zugangsdaten im Formular).

## Teil 1: Zweispaltige Struktur

Die Einstellungen zeigen links eine Navigation mit Kategorien, rechts den
Inhalt der gewählten Kategorie.

### 1.1 Kategorien

Eingebaut sind (in dieser Reihenfolge): KI-Provider, Anzeige & Sprache,
Diagnose, Dateien, Master-Passwort und Über. Eine Edition-Erweiterung kann
weitere Kategorien hinzufügen (siehe 1.2); sie stehen nach den eingebauten.

### 1.2 Registrierte Sektionen

Die Navigation rendert **auch** die über die Erweiterungs-Registry
(Spec 0038) registrierten Sektionen — als Navigationseintrag links und mit
ihrem Inhalt rechts, gleichwertig zu den eingebauten. Das ist der Weg, über
den eine Edition ihre Lizenz-Sektion einklinkt; sie darf nach einem Umbau
nie verschwinden.

### 1.3 Beschriftung registrierter Sektionen

Die Beschriftung einer registrierten Sektion ist optional; fehlt sie, wird
ein Standardtitel verwendet, sodass auch ältere Registrierungen ohne
Beschriftung sichtbar bleiben.

## Teil 2: Format-Hinweis für API-Keys

Im Anbieter-Formular erscheint beim Eingeben eines Keys sofort ein
offline berechneter Format-Hinweis.

- Präfix-Prüfung gegen den gewählten Anbieter: Anthropic `sk-ant-`, OpenAI
  `sk-`/`sk-proj-`, OpenRouter `sk-or-`. Passt es nicht, erscheint ein
  Hinweis („sieht nicht wie ein Anthropic-Key aus, erwartet `sk-ant-…`").
- **Warnung, keine Blockade.** Anbieter ändern Key-Formate; eine zu strenge
  Prüfung würde irgendwann gültige Keys ablehnen. Der Nutzer kann immer
  speichern.
- Für generische OpenAI-kompatible Anbieter gibt es **keine**
  Format-Prüfung (kein vorhersagbares Format; selbstgehostete Endpunkte
  dürfen nicht behindert werden).
- Die Prüfung läuft rein im Frontend ohne Netzwerk. Sie ergänzt das Trimmen
  der Eingabe (Spec 0049), das die eigentliche Lösung für eingeschleuste
  Zeilenenden bleibt.

## Teil 3: „Testen"-Knopf

Ein Knopf im Anbieter-Formular schickt mit den **gerade eingegebenen, noch
nicht gespeicherten** Daten einen minimalen Test-Request und zeigt das
Ergebnis direkt im Formular — wie „Verbindung testen" bei Servern
(Spec 0008).

- Der Test nutzt den Modell-Endpunkt (Spec 0025) bzw. einen minimalen
  Aufruf.
- Drei Ergebnisse werden unterschieden: **gültig**, **Authentifizierung
  fehlgeschlagen**, **nicht erreichbar**.
- Der Test-Request unterliegt derselben Redaction und Sicherheit wie
  reguläre Anbieter-Anfragen; der Key erscheint in keinem Log.

## Sicherheits- und Konsistenzzusagen

- Die Format-Prüfung blockiert nie das Speichern.
- Kein API-Key und kein Secret im Log, auch nicht beim Testen.
- Registrierte Sektionen (insbesondere die Lizenz-Sektion einer Edition)
  erscheinen weiterhin in der Navigation.
