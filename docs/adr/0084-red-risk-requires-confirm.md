# ADR 0084 — Rotes Risiko verlangt Bestätigung (Spec 0092)

Status: akzeptiert · Spec: `docs/specs/0092-red-risk-requires-confirm.md` ·
Backlog: BL-0074

## 1. Entscheidung

Eine neue app-weite Einstellung „Bei rotem Risiko immer nachfragen"
(`redRiskAlwaysConfirm` in `settings.json`), **Standard: an** — auch für
bestehende Installationen und bei einem unlesbaren oder nicht-booleschen
Wert (Fail-safe in Richtung „an", anders als bei der KI-Zweitmeinung selbst,
wo „im Zweifel aus" die sichere Richtung ist: dort droht eine
halbkonfigurierte KI-Anfrage, hier eine unbestätigte rote Aktion).

Ist sie an, macht ein neues Kettenglied
(`app-logic::orchestration::action_exec`) aus jedem `AutoExec` ein
`Confirm` mit Code `FILTER_RED_RISK_REQUIRES_CONFIRM`, sobald der
Risiko-Indikator **eine** der beiden Achsen (Server- oder Daten-Risiko) als
`Red` einstuft — auch gegen eine sonst greifende Allow-Regel, für Chat und
MCP gleichermaßen. Das Glied liest das Injection-Verdachts-Flag nur (wie
Secret-Pfad und `sftp-server`) und steht nach beiden, damit deren genauerer
Code erhalten bleibt.

**Nur Eskalation:** Kein Pfad aus Spec 0092 erzeugt `AutoExec` oder hebt ein
bestehendes `Deny`/`Confirm` auf. Ein Kommando, das bereits durch
Hard-Blacklist, Deny-Regel, Secret-Pfad oder `sftp-server` entschieden ist,
bleibt bei dieser Entscheidung.

## 2. Verhalten bei der KI-Zweitmeinung

`chat-action-proposed` geht immer mit der regelbasierten Einstufung hinaus.
Hebt die (optionale, nur für die Daten-Achse zuständige) KI-Zweitmeinung das
Daten-Risiko **nachträglich** auf Rot — nachdem ein Vorschlag also bereits
als „läuft automatisch" angekündigt war —, wird die eigentlich schon
angelaufene automatische Ausführung **nicht gestartet**. Stattdessen
durchläuft die Aktion denselben `Confirm`-Zweig wie jede andere
Bestätigung: dieselbe Wartezeit, dieselbe Abbruchlogik, derselbe
Hintergrund-Tab-Indikator, dieselben Ledger-Einträge.

Das Frontend erfährt das über ein neues Ereignis,
`action-decision-escalated` (`sessionId`, `actionId`, `reason`, `code`),
bewusst **nicht** als Erweiterung von `risk-assessment-updated`: Letzteres
kommt bei jedem Badge-Update, auch ohne Eskalation (z. B. `none`/`yellow`)
— eine Erweiterung hätte die Eskalation nur als Sonderfall mitgetragen.
`action-decision-escalated` kommt immer **nach** `risk-assessment-updated`
(Badge ist schon rot, wenn der Dialog erscheint) und **nach** der
Registrierung des Bestätigungs-Empfängers (ein sehr schneller Klick geht
nicht ins Leere). Das Frontend baut sich daraus selbst ein
`Decision::Confirm` zusammen, weil dieses Ereignis — anders als
`chat-action-proposed` — nicht den ganzen `Decision`-Enum überträgt.

Ein während der Zweitmeinung eingetroffener Stopp
(`auto_continue_stop`) hat Vorrang: Eine so gestoppte Aktion wird
übersprungen und bekommt keinen Dialog. Eine ehrlich benannte Restlücke
bleibt: Trifft der Stopp **nach** der Registrierung/dem Ereignis ein, sieht
der Nutzer noch kurz einen Dialog für eine bereits gestoppte Aktion — das
ist die sichere Richtung (eine Rückfrage zu viel, nie eine Ausführung zu
viel) und lässt sich nicht schließen, solange das Ereignis vor einem
möglichen Klick gesendet werden muss.

## 3. Der Ledger bekommt nie den freien Modelltext der Zweitmeinung

Spec 0092, §5, wörtlich gelesen, verlangt für den A3-Fall einen Grund „samt
Begründung" der KI-Zweitmeinung. Umgesetzt wurde stattdessen ein fester
Text ohne den Modelltext: „Daten-Risiko rot (KI-Zweitmeinung) – erfordert
immer Bestätigung".

Grund (Review-Fund, Runde 1, K2 in Spec 0092 §9): `Decision.reason` landet
über `handle_user_decision` unredigiert im persistierten Ledger —
`redact_ledger_entry_content` lässt `LedgerEntryContent::Decision` bewusst
unredigiert durch, mit der Begründung, dort stünden nur von der
Filter-Engine selbst erzeugte Texte. Der freie Text eines
KI-Zweitmeinungs-Modells kann dagegen zitieren, was es gerade beurteilt hat
— etwa ein Passwort aus dem Kommando —, und ist über Prompt-Injection
mittelbar fremdgesteuert. Den Text vor dem Speichern durch den Redactor zu
schicken wäre Scheinsicherheit gewesen: Der erkennt bekannte Secret-*Formen*
(API-Keys, Private Keys, Hashes, Bearer-Token), nicht ein beliebiges
Ad-hoc-Passwort. Die Begründung der Zweitmeinung geht dem Nutzer dabei nicht
verloren — sie steht weiterhin am Badge (`risk-assessment-updated.reason`),
nur nicht persistiert. Das Frontend zeigt diesen Text nur an; er darf nicht
in persistierten Zustand übernommen oder als HTML gerendert werden (derselbe
Grund: modellerzeugter, mittelbar fremdgesteuerter Text).

## 4. Die Grenze des Klassifizierers ist jetzt sicherheitsrelevant

Vor Spec 0092 war eine Lücke in den Muster-Listen des Risiko-Klassifizierers
(`core::risk::patterns`) „nur" eine unvollständige Warnung — der Indikator
war reine Anzeige. Jetzt hängt daran, ob die Eskalation aus Abschnitt 1
überhaupt greift: Ein Kommando, das inhaltlich riskant ist, aber keines der
hinterlegten Muster trifft, bleibt `None`/`None` eingestuft und läuft mit
Allow-Regel weiter automatisch, obwohl die neue Einstellung an ist.

Das ist **keine** Lockerung der Filter-Engine selbst — Hard-Blacklist und
Regel-Auswertung sind davon unberührt, ein hart blockiertes oder per Regel
verweigertes Kommando bleibt blockiert bzw. verweigert. Es ist eine reale
Grenze der zusätzlichen Sicherheitsschicht aus Spec 0092: Diese ist nur so
vollständig wie die Muster-Listen, die laut ihrem eigenen Modul-Kommentar
„Startpunkte, kein Anspruch auf Vollständigkeit" sind. Spec 0092 schließt
Änderungen am Klassifizierer selbst ausdrücklich aus (§3, Nicht-Ziele) — die
Muster-Pflege bleibt ein eigenes Thema.

Ebenso bewusst in Kauf genommen: Der Klassifizierer zählt **Bytes** und
bricht bei Überlänge ab (liefert dann „kein Risiko" im Sinn von „nicht
geprüft"), die Filter-Engine zählt **Zeichen**. Ein Kommando mit
Mehrbyte-Zeichen könnte deshalb unter der Zeichen-Schranke der Filter-Engine
liegen, während der Klassifizierer bereits aufgegeben hat. Das neue Glied
ist dafür selbst fail-safe gebaut (dieselbe Bytes-Schranke, dieselbe
`.len()`-Zählung wie der Klassifizierer) statt sich darauf zu verlassen,
dass ein Nachbarglied zufällig auch eskaliert — ein zu langes, nicht
einschätzbares Kommando gilt bei eingeschalteter Einstellung als Rot. Der
im Dialog gezeigte, feste übersetzte Text zu `FILTER_RED_RISK_REQUIRES_
CONFIRM` deckt deshalb beide Fälle ab ("rot eingestuft" **oder** "nicht
sicher einschätzbar"), nicht nur den regulären.

## 5. Rückfrage bei harmlosen, aber rot eingestuften Lesern

Eine unmittelbare Folge von Abschnitt 1: `cat ~/.ssh/id_rsa.pub` — der
**öffentliche** Teil eines SSH-Schlüsselpaars, unbedenklich zu lesen — wird
vom Risiko-Klassifizierer auf der Daten-Achse als Rot eingestuft (Muster
zielt auf den Pfad, nicht auf den Dateiinhalt) und ist **kein** Secret-Pfad
im Sinne der eigenen, präziseren Sonderregel dafür. Mit eingeschalteter
Einstellung verlangt dieser harmlose Lesebefehl jetzt trotzdem eine
Bestätigung. Bewusst akzeptiert: Die Alternative wäre eine Sonderregel für
"eigentlich unbedenkliche, aber musterbasiert rot eingestufte" Kommandos —
das widerspräche dem Entscheid aus Abschnitt 1 (eine der beiden Achsen rot
zählt, ohne Ausnahmeliste) und wäre genau die Art Sonderfall, die die
Muster-Pflege (s. Abschnitt 4) eigentlich vermeiden soll.

## 6. Die `accept_and_create_rule`-Falle

Der Schnellregel-Button im Bestätigungsdialog (`accept_and_create_rule`)
löst die aktuell wartende Bestätigung auf **und** legt eine neue Allow-Regel
an, für künftige Vorschläge. Bei einem rot eingestuften Kommando wirkt diese
neue Allow-Regel aber **nie**, solange die Einstellung aus Spec 0092 an
ist: Jeder künftige Vorschlag, der die Regel träfe, würde von
`evaluate_action` zwar auf `AutoExec` entschieden, aber vom neuen Glied aus
Abschnitt 1 sofort wieder auf `Confirm` zurückgehoben — derselbe rote
Risiko-Grund wie beim ersten Mal.

Für den Nutzer sieht das aus wie eine kaputte Regel ("ich habe doch eine
Allow-Regel angelegt"). Ist es nicht: Es ist dieselbe Eskalation wie bei
jeder anderen bereits bestehenden Allow-Regel auf ein rotes Kommando (Ziel
von Spec 0092, Abschnitt 1: "übersteuert auch eine Allow-Regel"). Bewusst
keine Sonderbehandlung für per Schnellregel gerade erst angelegte Regeln —
das wäre eine stille Ausnahme von genau der Garantie, die Spec 0092 gerade
einführt. Ob der Bestätigungsdialog das absehbar macht (z. B. ein Hinweis
"wird bei Rot trotzdem erneut gefragt"), ist eine Oberflächenfrage,
außerhalb dieses Schritts.

## 7. `EditThenApprove` bleibt außerhalb der Risiko-Kette

Altverhalten, von Spec 0092 nicht angefasst: Bearbeitet der Nutzer ein
Kommando im Bestätigungsdialog und klickt dann "Ausführen", gilt der Klick
selbst bereits als Bestätigung — der bearbeitete Text durchläuft zwar
erneut die Filter-Engine (damit eine per Bearbeitung eingeschleuste
gefährliche Änderung nicht durchrutscht), aber nicht mehr die
Risiko-Eskalation aus Abschnitt 1. Das ist konsistent mit dem übrigen
Verhalten dieses Pfads (Spec 0021: "der Klick *ist* die Bestätigung") und
war bereits vor Spec 0092 so — hier nur benannt, damit die Ausnahme nicht
stillschweigend unter den neuen Anspruch "jedes rote Kommando verlangt
Bestätigung" fällt.

## 8. Restfall nach dem Ausschalten (Spec 0092, §6)

Schaltet der Nutzer die Einstellung aus, gilt Rot wieder nur als Hinweis —
wie vor Spec 0092. Eine bereits laufende Sitzung behält ihren beim
`connect()` gelesenen Wert bis zur nächsten Verbindung (dasselbe Muster wie
die KI-Zweitmeinungs-Einstellung, Spec 0026). Der Hinweistext am Schalter
sagt das. Bewusst kein Live-Neulesen pro Vorschlag: Das hätte einen
zusätzlichen Store-Zugriff auf jedem Ausführungspfad bedeutet, für eine
Einstellung, die typischerweise selten geändert wird.
