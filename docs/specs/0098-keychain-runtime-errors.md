# Spec 0098 — Schlüsselbund-Fehler zur Laufzeit ohne Bibliothekstext

Status: umgesetzt
Zweck: Jeder Fehler des Schlüsselbunds, der nach dem Start auftritt, erreicht die Oberfläche als übersetzte Meldung mit stabilem Code und ohne den Text der Schlüsselbund-Bibliothek, und eine einzelne Ablehnung ändert den Verfügbarkeitszustand nicht.
Bezüge: Spec 0071 (Schlüsselbund beim Start, A14–A17, X2), Spec 0094 (Logregeln), Spec 0076 (Hop-Angabe, A-8), Spec 0101 (Schlüsselbund hält den Wurzelschlüssel).
Review-Priorität: ERHÖHT

## 1. Verhalten im Überblick

- Ist der Schlüsselbund beim Start nicht verfügbar, melden alle Wege, die
  ihn brauchen, `KEYCHAIN_UNAVAILABLE` (Spec 0071).
- Scheitert ein Zugriff später **bei verfügbarem** Schlüsselbund (gesperrt,
  Dialog abgelehnt, Plattformfehler), meldet jeder Weg `KEYCHAIN_ACCESS_FAILED`
  mit einer festen, übersetzten Meldung. Der Text der Bibliothek erreicht die
  Oberfläche nicht, auch nicht in einem Detailfeld.
- Der Verfügbarkeitszustand aus dem Start bleibt unverändert; es gibt weder
  eine nachträgliche Eskalation noch eine Anpassung. Ein einzelner
  Fehler macht den Schlüsselbund nicht „nicht verfügbar", und der nächste
  Zugriff gelingt, sobald der Speicher wieder antwortet.
- Der Verbindungsaufbau und der Verbindungstest unterscheiden Schlüsselbund-
  Fehler von fehlenden Einträgen und von Netzwerkfehlern, auch auf Jump-Hosts.

## 2. Hintergrund

Die eingesetzten Schlüsselbund-Anbindungen unterscheiden „gesperrt" nicht
verlässlich: Auf macOS sehen ein gesperrter Schlüsselbund, ein abgebrochener
Dialog und ein „Nicht erlauben" gleich aus; unter Windows und Linux gibt es nur
wenige eigene Fehlerarten. Deshalb behandelt die Anwendung alle Arten gleich
und zeigt keine Unterscheidung nach Ursache.

## 3. Ziel und Nicht-Ziele

Ziel: Kein Schlüsselbund-Fehler zur Laufzeit erreicht die Oberfläche mit dem
Text der Bibliothek. Der Nutzer sieht, dass der Schlüsselbund die Ursache ist.

Nicht-Ziele:

- Kein erneutes Prüfen der Verfügbarkeit zur Laufzeit; der Zustand aus dem
  Start bleibt die einzige Quelle für `KEYCHAIN_UNAVAILABLE` (Spec 0071 A16).
- Keine Unterscheidung nach Fehlerart (gesperrt / abgelehnt / Plattform).
- Stellen, die den Fehler heute bewusst verwerfen (z. B. optionale Lesevorgänge
  mit Protokoll-Warnung), bleiben, wie sie sind.
- Ein fehlender Eintrag (`NotFound`) bleibt „kein Eintrag" (Spec 0071 A14, I4).
- Kein neuer Verfügbarkeitszustand und keine Änderung der Diagnose-Ansicht.

## 4. Anforderungen

- **A1** Ein Backend-Fehler des Schlüsselbunds bei verfügbarem Schlüsselbund
  erreicht die Oberfläche mit dem stabilen Code `KEYCHAIN_ACCESS_FAILED` und
  einer festen Meldung ohne die Nutzlast des Fehlers. Das gilt für alle Wege,
  lesend wie schreibend, **und für das Entfernen des Sudo-Passworts**. Das
  ändert Spec 0071 A17: Bei verfügbarem Schlüsselbund meldet dieser Weg
  `KEYCHAIN_ACCESS_FAILED` statt `KEYCHAIN_UNAVAILABLE`.
- **A2** Bei nicht verfügbarem Schlüsselbund bleibt es bei
  `KEYCHAIN_UNAVAILABLE`.
- **A3** Ein Fehler zur Laufzeit ändert den Verfügbarkeitszustand nicht. Nach
  einem einzelnen fehlgeschlagenen Zugriff gelingt der nächste, sobald der
  Speicher wieder antwortet, und keine Meldung behauptet „nicht verfügbar".
- **A4** Scheitert beim Verbindungsaufbau oder Verbindungstest das Lesen eines
  Secrets mit einem Backend-Fehler, auf jedem Hop einschließlich der
  Jump-Hosts, trägt das Ergebnis einen Code, der den Schlüsselbund als Ursache
  nennt: `KEYCHAIN_ACCESS_FAILED` bei verfügbarem, `KEYCHAIN_UNAVAILABLE` bei
  nicht verfügbarem Schlüsselbund. Es steht nicht unter „Netzwerkfehler" und
  nicht als „Zugangsdaten konnten nicht aufgelöst werden". Fehlt der Eintrag
  (`NotFound`), bleibt es bei `SSH_CREDENTIAL_RESOLUTION_FAILED`.
- **A5** In den Fällen aus A1 und A4 enthält kein Feld, das zur Oberfläche
  geht (`message` eingeschlossen), den Text der Bibliothek. Erlaubt sind die
  Art des Secrets („Passwort", „Passphrase" …), feste Texte und die
  Hop-Angabe `user@host:port` aus Spec 0076 A-8.
- **A6** `KEYCHAIN_ACCESS_FAILED` ist in DE und EN übersetzt und der
  Oberfläche als bekannter Code bekannt. Inhalt: Der Zugriff auf den
  Schlüsselbund ist fehlgeschlagen, möglicherweise ist er gesperrt oder der
  Zugriff wurde abgelehnt; erneut versuchen; der genaue Grund steht im Log.
  Ohne „nicht verfügbar" und ohne Paket- oder Installationshinweise.
- **A7** Der Text der Bibliothek bleibt zur Diagnose im Log auf Stufe `debug`
  erhalten; es gelten die Regeln aus Spec 0094. Bestehende Warnungen mit dem
  Fehler bleiben unverändert; Spec 0094 nimmt Schlüsselbund-Fehler aus.

## 5. Weg des Fehlers

- Die Unterscheidung zwischen A1 und A2 fällt dort, wo der Verfügbarkeitszustand
  bekannt ist; die fachliche Kernlogik hängt nicht von ihm ab.
- Der Fehler bleibt vom Lesen des Secrets bis zur Antwort als
  Schlüsselbund-Fehler erkennbar, ohne seine Nutzlast mitzuführen.
- Die Form der Fehlerantworten ändert sich nicht, es kommt nur ein Code-Wert
  hinzu.

## 6. Sicherheitszusagen

- **Spec 0071 X2 (kein Secret über die Fehlerkette)** gilt erweitert: Die
  Nutzlast des Backend-Fehlers erreicht die Oberfläche auf keinem Weg aus
  A1/A4, auch nicht im Feld `message`.
- **Spec 0071 A14/I4:** `NotFound` bleibt getrennt von Backend-Fehlern.
- **Spec 0071 A16:** ein Zustand je Programmlauf (A3).
- **Spec 0094:** neue Log-Einträge nur auf `debug` (A7).
- Das Lesen und Schreiben im Credential-Speicher ändert sich nicht, nur die
  Darstellung des Fehlers.

## 7. Testfälle

Die Nutzlast ist in jedem Test ein Marker wie `LIBTEXT-0098 Geheim-0098`.
Für den Verbindungsweg gibt es zwei Ebenen: V1 prüft Jump-Hosts im
Verbindungstest mit einem Connector, der die echte Auflösung der Secrets
aufruft; V2 prüft den ersten Hop gegen den Testserver der Transportschicht.

- **T1 (A1, Schreiben):** Server-Passwort speichern, Speicher scheitert,
  Schlüsselbund verfügbar → Code `KEYCHAIN_ACCESS_FAILED`, Meldung ohne Marker.
- **T2 (A1, Lesen):** ein lesender Weg (Verbindungstest oder Schlüsseldatei) →
  wie T1.
- **T3 (A2):** dieselben Fälle bei nicht verfügbarem Schlüsselbund →
  `KEYCHAIN_UNAVAILABLE`. **T3a:** Entfernen des Sudo-Passworts scheitert:
  verfügbar → `KEYCHAIN_ACCESS_FAILED`, nicht verfügbar →
  `KEYCHAIN_UNAVAILABLE`.
- **T4 (A3):** Erst scheitert der Speicher, dann gelingt er. Der erste Fehler
  trägt nicht `KEYCHAIN_UNAVAILABLE`, der zweite Zugriff gelingt, der Zustand
  bleibt „verfügbar".
- **T5 (A4, Jump-Host, V1):** Verbindungstest mit einem Jump-Host, dessen
  Passwort-Lesen mit Backend-Fehler scheitert → `KEYCHAIN_ACCESS_FAILED` bzw.
  `KEYCHAIN_UNAVAILABLE`, `message` ohne Marker.
- **T6 (A4, Ziel-Hop und Aufbau):** Der Ziel-Hop löst sein Secret **vor** der
  Kette auf (ein gefülltes Formularfeld liegt im flüchtigen Speicher und kann
  nicht scheitern; ein leeres Feld liest das gespeicherte Secret und liefert
  den Fehler als Befehlsfehler wie T2). V1 gilt daher nur für Jump-Hosts; für
  den ersten Hop gilt V2, für den Verbindungsaufbau die Abbildung des Fehlers
  auf den Code.
- **T7 (A4, NotFound):** fehlender Eintrag auf einem Jump-Host →
  `SSH_CREDENTIAL_RESOLUTION_FAILED`.
- **T8 (A6):** `KEYCHAIN_ACCESS_FAILED` ist bekannt und in DE/EN übersetzt;
  die Anzeige des Verbindungstests zeigt bei diesem Code weder
  „Netzwerkfehler" noch den Marker.
- **T9 (adversarial):** Nutzlast mit Zeilenumbruch und Anführungszeichen →
  in keinem Feld zur Oberfläche.
- **T10:** Nutzlast, die selbst wie ein Code aussieht (`KEYCHAIN_UNAVAILABLE`)
  → Code bleibt `KEYCHAIN_ACCESS_FAILED`; die Nutzlast fließt nicht in die
  Code-Wahl ein.
- **T11:** Fehler beim Löschen auf einem Weg, der ihn anzeigt → wie T1.
- **T12 (V1):** Jump-Host-Kette mit drei Hops, Fehler am mittleren → Code nach
  A4, kein Marker. **T12a (V2):** Am ersten Hop bleibt die Hop-Angabe nach
  Spec 0076 A-8 in der Meldung.
- **T13:** Passphrase eines Private Keys auf dem Jump-Host scheitert → A4
  gilt für alle Secret-Arten, nicht nur für „Passwort".

## 8. Grenzen

- Der Bericht zu T6 nennt, was ohne echte Verbindung nicht abgedeckt ist:
  Fehler ab dem zweiten Hop sind gegen den Testserver nicht zuverlässig
  erreichbar (ADR 0008).
- Wege, die einen Fehler bewusst verwerfen, zeigen keinen Text und bleiben
  unverändert (Abschnitt 3).
