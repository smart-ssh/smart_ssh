# 0024-risk-indicator-second-opinion-design

## Status
Vorgeschlagen

## Kontext

`docs/specs/0026-command-risk-indicators.md` gibt den Trait, die
Eskalationsregel ("nur Eskalation, nie Abschwächung") und die
Event-Skizze vor, lässt aber offen, wie die optionale KI-Zweitmeinung
konkret in die bestehende `run_chat_turn`/`run_one_round`/
`handle_action_proposed`-Aufrufkette (Spec 0007/0021) eingehängt wird.

## Entscheidungen

**1. Zweitmeinung läuft inline in `handle_action_proposed` (nach dem
bereits gesendeten Event), kein losgelöster `tokio::spawn`-Task.** Ein
echter `spawn` hätte `Arc<Session>`/`Arc<dyn EventEmitter>` gebraucht, um
die 'static-Anforderung zu erfüllen — `run_chat_turn`/`run_one_round`/
`handle_action_proposed` nehmen aber bewusst `&Session`/`&dyn EventEmitter`
(s. deren eigene Doc-Kommentare zur Testbarkeit gegen `MockAiProvider`
ohne echte Tauri-Runtime), und über 40 bestehende Testfälle konstruieren
`Session` als lokalen, teils nachträglich mutierten Wert (`session.filter_
engine = ...`), nicht als `Arc`. Ein Signatur-Umbau für einen einzelnen
zusätzlichen `.await` wäre unverhältnismäßig gewesen. Der Unterschied zur
"asynchron"-Vorgabe aus Spec 0026 Abschnitt 3 ist rein intern: das
`chat-action-proposed`-Event geht über Tauris IPC-Eventbus sofort raus,
unabhängig davon, wie lange die umschließende Rust-Funktion danach noch
läuft — das Frontend sieht das Badge exakt so schnell, wie es das bei
einem echten `spawn` auch täte. Einziger realer Unterschied: reicht die
Zweitmeinungs-Anfrage in die Confirm-Warte-Phase hinein, verzögert sich
die *Verarbeitung* eines bereits erfolgten Nutzerklicks um die Restlaufzeit
dieser Anfrage (der Klick selbst geht nicht verloren, er liegt im Kanal
bereit).

**2. Der Zweitmeinungs-Provider wird einmalig bei `connect()` aufgelöst
und auf `Session` abgelegt, nicht live pro Aktionsvorschlag neu gelesen.**
Dieselbe Signatur-Stabilitäts-Begründung wie oben: eine Live-Auflösung
hätte `AppHandle`/`AppState`-Zugriff bis tief in `handle_action_proposed`
gebraucht. Konsequenz: Ändert der Nutzer die Einstellung mitten in einer
offenen Session, wirkt das erst nach einem erneuten `connect()` — für eine
reine Komfort-Einstellung (kein Sicherheitsschalter, die regelbasierte
Einschätzung bleibt in jedem Fall aktiv) ein akzeptabler Kompromiss.

**3. Escalation-Logik als reine Funktion (`escalate_data_risk`), getrennt
von der Event-/Async-Maschinerie.** Macht den von der Aufgabenstellung
explizit verlangten Test ("ein regelbasiertes Red bleibt Red, auch wenn
die KI none zurückgibt") direkt und ohne Event-Mitschnitt prüfbar — plus
zusätzlich end-to-end über `run_chat_turn` samt `TestEmitter` abgesichert,
damit die Verdrahtung selbst (nicht nur die Formel) getestet ist.

**4. Zweitmeinungs-Antwort-Parsing über eine wortweise Suche nach `none`/
`yellow`/`red`, keine reine Teilstring-Suche.** Ein `text.contains("red")`
würde auf z. B. "redirect" fehltriggern. Stattdessen wird jedes einzelne
(satzzeichenbereinigte) Wort geprüft — das erste, das exakt einem der drei
Level entspricht, gewinnt; alles danach wird als Begründung übernommen,
mit Fallback auf den vollen Antworttext, falls danach nichts Sinnvolles
mehr übrig bleibt. Fehlschlägt das Parsen komplett, liefert die Funktion
`None` ("keine Zweitmeinung verfügbar") statt eines Fehlers — im selben
Geist wie das Fallback-Tool-Calling-Parsing aus Spec 0006.

**5. Risiko-Einstellungen (`riskClassifierEnabled`/
`riskClassifierProviderId`) liegen im selben `tauri-plugin-store`-File wie
die UI-Sprache (`settings.json`, Spec 0024).** Beides sind reine
App-Einstellungen ohne Bezug zu Server-/Gruppen-Fachdaten — dieselbe
Begründung wie in Spec 0024 bereits etabliert, keine neue SQLite-Migration
für eine einzelne, nicht sicherheitsrelevante Konfiguration.

**6. Der Klassifizierer prüft zusätzlich zu den einzelnen Segmenten auch
das unzerlegte Gesamtkommando.** `filter::parser::scan_top_level_segments`
verfolgt nur `(`/`)`, keine `{`/`}` — ein Muster wie die klassische
Fork-Bombe `:(){ :|:& };:`, deren `|`/`;` innerhalb der `{}`-Klammern
liegen, wird deshalb an genau diesen Zeichen mit-zerlegt, obwohl es
fachlich ein einzelnes Kommando ist. Statt diese vorbestehende
Parser-Eigenart zu ändern (außerhalb des Scopes dieser Spec, betrifft auch
die Filter-Engine selbst), prüft `RuleBasedRiskClassifier::classify`
zusätzlich das gesamte, unzerlegte Kommando gegen beide Musterlisten.

## Konsequenzen

**Positiv:**
- Kein invasiver Umbau der bestehenden, gut getesteten
  Orchestrierungs-Signaturen für ein optionales Zusatzfeature.
- Die Kernsicherheitseigenschaft ("Red bleibt Red") ist sowohl als reine
  Funktion als auch end-to-end abgesichert.
- Robustes Zweitmeinungs-Parsing ohne Fehltrigger auf Teilstrings.

**Negativ / Trade-off:**
- Ein während einer offenen Session umgeschalteter Zweitmeinungs-Provider
  wirkt erst nach erneutem Verbindungsaufbau — dokumentiert, aber ein
  echtes Live-Verhalten wäre für Nutzer:innen ohne diesen Hintergrund
  potenziell überraschend.
- Bei einer im Confirm-Dialog bereits ausstehenden Zweitmeinungs-Anfrage
  kann ein sehr schneller "Ausführen"-Klick des Nutzers um die Restlaufzeit
  dieser Anfrage verzögert tatsächlich verarbeitet werden (typischerweise
  im Bereich weniger Sekunden, begrenzt durch denselben
  SSE-Inaktivitäts-Timeout wie reguläre Chat-Anfragen).
