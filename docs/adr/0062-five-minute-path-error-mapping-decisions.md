# ADR XXXX: Entscheidungen bei der Umsetzung von Spec 0069 (Fünf-Minuten-Pfad: Einstieg und verständliche Fehler)

Status: Angenommen
Bezug: docs/specs/0069-five-minute-path-onboarding-and-errors.md, Commits
`b134075` (A3), `31dab1e` (A2), `b55eba7` (A4), `5132657` (A5), `0e18508`
(B), `fbe15c9` (C1), `89cf360` (C2), Review-Nacharbeit (s. Abschnitt 4)

Diese Spec verlangt ausdrücklich ein ADR für zwei Design-Entscheidungen
(Abschnitt 1/2 unten) und — laut Coder-Arbeitsweise — eines für jeden im
Abschluss-Review bewusst nicht behobenen Fund (Abschnitt 3).

## 1. Loopback-Erkennung statt Provider-Typ (Teil A2)

**Frage:** Wie unterscheidet der Fehler-Mapper `crates/ai-providers/src/
error.rs::map_transport_error` "lokaler KI-Dienst nicht gestartet"
(`AI_LOCAL_PROVIDER_UNREACHABLE`) von "entfernter Provider nicht
erreichbar" (`AI_NETWORK_ERROR`)?

**Entscheidung:** An der Ziel-Adresse des gescheiterten Requests
(`err.url()`, Host ist `localhost` oder eine Loopback-IP), nicht am
konfigurierten `ProviderType`.

**Begründung:** Der Provider-Typ `ollama` sagt nichts darüber, *wo* die
Instanz läuft — ein Ollama auf einem anderen Rechner im Netz ist ein
entfernter Endpunkt wie jeder andere und sollte nicht fälschlich "starte
Ollama (auf diesem Rechner)" vorschlagen. Umgekehrt kann ein beliebiger
anderer OpenAI-kompatibler Dienst (LM Studio o. Ä.) lokal auf
`127.0.0.1` laufen und profitiert von derselben Meldung, ohne dass das
Mapping seinen Typ kennen müsste. Die Alternative (Unterscheidung nach
`ProviderType` in `app-shell`) wurde verworfen — sie bräuchte den Typ an
jeder Fehlerstelle und wäre in beiden Richtungen (false positive bei
entferntem Ollama, false negative bei lokalem Nicht-Ollama-Endpunkt)
ungenauer.

**Konsequenz:** `is_loopback_target` erkennt `localhost` (Groß-/
Kleinschreibung ignoriert) sowie IPv4-/IPv6-Loopback-Literale exakt.
Nicht erkannt werden `localhost.` (mit Punkt), IPv4-mapped IPv6
(`::ffff:127.0.0.1`) oder ein DNS-Name, der auf `127.0.0.1` auflöst
(reines String-/IP-Literal-Matching, keine Auflösung). Das ist ein
reiner Label-Ungenauigkeit-Fall (der Nutzer sieht "Internetverbindung
prüfen" statt "starte Ollama"), keine Sicherheitswirkung — bewusst nicht
weiter verallgemeinert, s. Abschnitt 3.

## 2. Nachträgliche DNS-Diagnose statt Vorab-Auflösung (Teil A3, ERHÖHT)

**Frage:** Wie liefert `ssh_transport::connect()` den Code
`SSH_HOST_NOT_FOUND`, wenn der eingetragene Hostname nicht auflösbar
ist?

**Entscheidung:** Nur wenn der erste Hop mit der generischen
`SshError::ConnectionFailed`-Variante scheitert, prüft `connect()`
zusätzlich per `tokio::net::lookup_host((host, port))`, ob der Name
überhaupt auflösbar ist — **nach** dem gescheiterten Versuch, nicht
davor.

**Begründung:** Der Verbindungsaufbau selbst (`client::connect`) und der
Host-Key-Abgleich (`ClientHandler { host, .. }`, `resolve_or_pending`)
reichen `first_hop.host` unverändert an `russh` durch — genau wie vor
dieser Spec. Eine Vorab-Auflösung mit anschließender Übergabe der IP an
`russh` hätte eine DNS-Anfrage im Fehlerfall gespart, aber den
sicherheitsrelevanten Verbindungs-/Host-Key-Pfad verändert (Host-Keys
sind unter dem eingegebenen Hostnamen gespeichert, nicht unter einer
IP) — genau die Art stiller Verengung, die diese Spec ausdrücklich
ausschließt. Die nachträgliche Diagnose ist dafür in einem Fehlerfall
minimal langsamer (eine zusätzliche DNS-Anfrage), ändert aber am
Erfolgs- und Host-Key-Pfad nichts — durch den spec-reviewer bestätigt
(Frage 2 der ERHÖHTEN Prüfung, s. Bericht).

**Konsequenz:** Gilt nur für den **ersten** Hop; Jump-Hosts ab dem
zweiten Hop bleiben unverändert bei `ConnectionFailed` (Spec §2,
Nicht-Ziele). Ein DNS-Wettlauf (Name löst beim eigentlichen Connect noch
auf, ist beim nachträglichen `lookup_host` aber bereits verschwunden,
oder umgekehrt) führt schlimmstenfalls zu einem falschen Code-Label
(`SSH_HOST_NOT_FOUND` statt `SSH_CONNECTION_FAILED` oder umgekehrt) —
kein Sicherheitsrisiko, da in keinem Fall ein Host-Key akzeptiert oder
übersprungen wird.

## 3. Im Abschluss-Review bewusst nicht behobene Funde

Der `spec-reviewer` (ERHÖHT, volle Commit-Range) fand keinen
sicherheitsrelevanten Verstoß — die drei gezielten Fragen zur
Host-Key-Invariante wurden alle positiv beantwortet (kein Timeout und
keine Fehlklassifizierung kann einen Host-Key akzeptieren, eine Abfrage
überspringen oder die Bestätigungswartezeit verkürzen; Erfolgs- und
Host-Key-Pfad sind byte-gleich zum bisherigen Verhalten). Folgende
SOLLTE-Funde wurden geprüft und bewusst **nicht** verändert:

- **`map_russh_error`s neue Varianten gelten auch für laufende Sitzungen,
  nicht nur den Verbindungsaufbau.** Dieselbe Funktion wird — wie schon
  vor dieser Spec — auch für `channel_open_direct_tcpip` und (über
  `map_transport_error`) für Exec-/SFTP-Fehler einer bereits verbundenen
  Sitzung genutzt. Ein Verbindungsabbruch mitten in der Sitzung liefert
  jetzt z. B. `SSH_CONNECTION_CLOSED` mit dem Text "… während des
  Aufbaus beendet", was für diesen Fall sachlich falsch ist. **Nicht
  behoben:** Spec 0069 §2 (Nicht-Ziele) schließt Fehlermeldungen
  außerhalb des Fünf-Minuten-Pfads ausdrücklich aus; eine sauberere
  Lösung (Unterscheidung nach Aufrufkontext: Verbindungsaufbau vs.
  laufende Sitzung) wäre ein größerer, hier nicht beauftragter Umbau.
  Keine Sicherheitswirkung (kein Codepfad verzweigt auf diese Varianten,
  s. Review); nur Meldungsqualität außerhalb des beauftragten Pfads.
  Folge-Item empfohlen.
- **Der neue `SSH_CONNECT_TIMEOUT` (10 s) umschließt auch die
  Authentifizierung und die gesamte Hop-Kette**, nicht nur den
  TCP-Handshake — spec-konform (§3.A3 verlangt wörtlich, dass er *jeden*
  `ssh_transport::connect`-Aufruf umschließt, und `connect()` enthält
  `authenticate(...)` für jeden Hop). **Nicht behoben, weil spec-treu:**
  eine Änderung hier wäre eine Abweichung von der freigegebenen Spec,
  keine Fehlerbehebung. Konsequenz für den Nutzer: ein SSH-Agent-Key mit
  Bestätigungspflicht (`ssh-add -c`) oder ein FIDO/sk-Schlüssel mit
  Touch-Anforderung, die länger als 10 s Interaktion brauchen, sowie eine
  hochlatente mehrstufige Jump-Host-Kette, brechen jetzt mit
  `SSH_TIMEOUT` ab, wo vorher (ohne jeden Timeout) gewartet wurde. Fail
  sichtbar im UI, keine Invariante verletzt. Falls das in der Praxis
  stört, ist das eine Produktentscheidung (höherer Timeout, oder
  Ausklammern der Auth-Phase) — gehört Stefan.
- **IPv6-/Trailing-Dot-Sonderfälle bei der Loopback-Erkennung** (s.
  Abschnitt 1 oben) — bewusst nicht erweitert, reine Label-Ungenauigkeit
  ohne Sicherheitswirkung, außerhalb des von der Spec vorgegebenen
  `127.0.0.1`-Literals (§2, Nicht-Ziele: "Ollama auf anderem Port/Host …
  nur `127.0.0.1:11434`").
- **`aiProvider.ollamaProviderName` ist lokalisiert** ("Ollama (local)"
  in der englischen UI), während Spec B4.1 den Namen als festen String
  "Ollama (lokal)" nennt. **Bewusst nicht angepasst:** konsistent mit
  jedem anderen neuen Text dieser Spec (DE/EN-Paar statt fester
  deutscher String), und der bessere Produktentscheid für eine englische
  UI. Geringe Tragweite (reiner Anzeigename, vom Nutzer jederzeit
  umbenennbar).
- **DNS-Tests (`.invalid`-Domain) hängen vom lokalen Resolver ab** — in
  einem Netz mit Wildcard-DNS/Captive-Portal könnten
  `test_connect_to_unresolvable_host_yields_host_not_found`
  (`ssh-transport/tests/integration.rs`) und
  `test_non_loopback_dns_failure_stays_network_error`
  (`ai-providers/src/error.rs`) fehlschlagen. Vorbestehendes
  Test-Design-Risiko (dieselbe Technik wird bereits an anderer Stelle im
  Projekt genutzt), nicht neu durch diese Spec eingeführt — nicht
  behoben, Backlog-Kandidat: injizierbare Lookup-Funktion statt echtem
  Resolver.
- **`ManagementView.tsx`s Platzhaltertext ("Links eine Gruppe oder …")
  bleibt fest deutsch.** Liegt im selben Bildschirm wie die von Teil C1
  lokalisierten `ServerList`-Strings, war aber nicht Teil des
  beauftragten Dateisatzes dieser Spec — nicht angefasst, um den Scope
  nicht eigenmächtig zu erweitern.
- **Host-Key wird vor dem erfolgreichen Reconnect persistiert
  (`trust()` vor dem zweiten `connect()`-Versuch).** Vorbestehendes
  Verhalten aus ADR 0007 ("fortgesetzt" heißt dort bewusst "frischer
  Reconnect, kein Pausieren"), durch den neuen 10-s-Timeout nur
  deterministischer auslösbar (ein Angreifer, der nach dem
  Fingerprint-Dialog künstlich verzögert, kann einen dauerhaft
  vertrauten Key ohne je eine erfolgreiche Sitzung erzielen). Kein neuer
  Fund dieser Spec — eigenes Item wert, ob `trust()` erst nach
  erfolgreichem Reconnect greifen sollte; hier nicht angefasst.

## 4. Nicht als eigener ADR-Punkt, aber im Review vermerkt

`docs/specs/0069-five-minute-path-onboarding-and-errors.md` trägt in
seinem Kopf eine `Gate:`-Zeile mit HQ-internen Release-Gate-Kennungen
(`pre-release-0x/D`, `release-1.0/B`, `release-1.0/C`) — die einzige
Spec in diesem öffentlichen Repo mit einer solchen Zeile. Sie stammt aus
dem Spec-Commit selbst (vor dem Implementierungs-Schritt dieser ADR),
wird hier vom Coder nur zur Kenntnis gebracht, nicht verändert — Umfang
und Entscheidung, ob interne Gate-Kennungen künftig aus öffentlichen
Specs herausgehalten werden, liegen beim Architekten/Stefan.

## Verworfene Test-Abdeckung (dokumentiert statt stillschweigend fehlend)

- **Test 22** (Spec §6: "App-Start ohne geöffnete Einstellungen löst
  keinen `discoverModels`-Aufruf aus") ist nicht als eigener Test
  umgesetzt. Die Eigenschaft hält strukturell (`App.tsx`: `SettingsScreen`
  wird nur bei `settingsOpen === true` gerendert, `AiProviderSettings`
  nur innerhalb davon) und wurde vom spec-reviewer bestätigt. Ein echter
  Test dafür bräuchte einen vollständigen `App.test.tsx`-Harness (Mocks
  für `useSessionTabs`, alle `ServerList`-/`App`-API-Aufrufe) — für diese
  eine Eigenschaft unverhältnismäßig; zurückgestellt, bis ein
  App-Level-Testrahmen aus einem anderen Anlass entsteht.
