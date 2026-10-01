# ADR 0089 — Schlüsselbund-Fehler zur Laufzeit: offene Entscheidungen und bewusst nicht behobene Funde

Status: akzeptiert · Spec: `docs/specs/0098-keychain-runtime-errors.md`

Spec 0098 ließ den Weg von `core` bis zum DTO ausdrücklich offen (§5) und
verlangte nur, dass `core` ohne `AppState`-Abhängigkeit bleibt. Hier stehen
die getroffenen Entscheidungen und die Funde des Reviews, die bewusst nicht
behoben wurden.

## 1. Eine eigene `SshError`-Variante ohne freies Textfeld

**Entscheidung:** `SshError::CredentialStoreFailed { secret: SecretKind, hop:
Option<HopLabel> }` statt eines Codes am bestehenden
`CredentialResolutionFailed`.

**Warum nicht der bestehende Fehler mit Zusatzcode:** Die Variante trägt
einen `String`. Solange sie das tut, ist „die Nutzlast der Bibliothek steht
nicht darin" eine Zusage, die bei jeder künftigen Änderung neu eingehalten
werden muss. Die neue Variante hat **kein** Feld für freien Text: `SecretKind`
ist ein Enum mit fünf Werten, `HopLabel` sind drei getrennte Felder. Die
Zusage aus A5 hält damit per Konstruktion — eine Umgehung wäre kein Versehen
mehr, sondern eine neue Variante.

**Preis:** Eine neue öffentliche Enum-Variante. §5 erlaubt das ausdrücklich
(„ändert keine öffentliche Schnittstelle" im Sinne der DTO-Form). Exhaustive
`match`-Stellen außerhalb dieses Repos — der Pro-Code — können davon brechen;
siehe Abschnitt 6.

## 2. `core` vergibt die vorsichtigere der beiden Codes

`SshError::code()` liefert für die neue Variante `KEYCHAIN_ACCESS_FAILED`,
nicht `KEYCHAIN_UNAVAILABLE`. `core` kennt den Startzustand nicht und könnte
„nicht verfügbar" nur raten.

Die Richtung ist bewusst gewählt: Erst die Schicht, die `AppState.keychain`
kennt (`keychain_aware_ssh_error_code`), darf daraus `KEYCHAIN_UNAVAILABLE`
machen. Vergisst ein künftiger Aufrufer diesen Schritt, ist das Ergebnis **zu
vorsichtig, nicht zu dreist** — er meldet „dieser Zugriff ist gescheitert"
statt der stärkeren Behauptung „der Schlüsselbund ist weg". Die umgekehrte
Vorgabe hätte ein vergessenes Mapping in eine Falschaussage verwandelt.

## 3. Die Hop-Benennung wohnt in `core`, nicht in `ssh-transport`

`SshError::named_for_hop` steht in `core`, obwohl nur
`ssh_transport::auth::name_hop` sie produktiv aufruft.

Grund: Fehler ab dem **zweiten** Hop sind gegen einen echten Testserver nicht
erreichbar (ADR 0008, Spec 0098 §7). Der Verbindungstest braucht dieselbe
Benennung deshalb auf Ebene V1. Zunächst war sie dort **nachgebaut** — und
ein Nachbau prüft sich selbst: Hörte `name_hop` auf, die neue Variante zu
benennen, wären die Hop-Zusicherungen in T5/T12/T13 grün geblieben (Fund des
spec-reviewers, Runde 1). Jetzt rufen beide Seiten dieselbe Funktion, und ein
solcher Rückfall macht T5 und T12 rot (nachgewiesen durch Probe).

## 4. Redaction auf einer `debug`-Zeile, die A7 davon ausnimmt

Spec 0098 A7 nimmt Schlüsselbund-Fehlerwerte ausdrücklich von Spec 0094 aus,
und die in §1 vermessenen Stores bilden ihre Nutzlast nicht aus dem
abgelehnten Wert. Die beiden neuen `debug`-Zeilen schicken sie trotzdem durch
`default_log_redactor()`.

Grund: A5 erklärt genau diese Nutzlast für unzustellbar, und ein Log ist eine
Datensenke. Spiegelt ein künftiger Store (oder eine neue `keyring`-Version)
den Wert doch in seiner Fehlermeldung, stünde er bei `RUST_LOG=debug` im
Klartext in der Datei. Die Zeile kostet nichts und ist die Form, die der Rest
der Codebasis ohnehin verwendet. **Eine Verschärfung über die Spec hinaus,
keine Abweichung.**

## 5. Bewusst nicht behobene Funde des Reviews (Runde 1)

- **T3a, Fall „nicht verfügbar", war schon am alten Stand grün.** Richtig — er
  ist kein Regressionstest, sondern die A2-Gegenprobe zu T3a: Er hält fest,
  dass die Umstellung auf `KEYCHAIN_ACCESS_FAILED` den Fall „beim Start weg"
  **nicht** mitgenommen hat. Als solcher bleibt er stehen. Dass er nichts über
  den alten Stand sagt, steht im Bericht.
- **Zwei Tests in `map_connect_result_tests` werden unter keiner Probe rot.**
  Sie konstruieren den `SshError` von Hand und prüfen nur die Abbildung. Ihr
  echter Gegenbeweis wäre „die Variante existierte vorher nicht" — das lässt
  sich nicht als Probe formulieren, weil die Tests sich dann nicht übersetzen
  lassen. Der Verhaltensbeweis für denselben Pfad sitzt in T12a (echte
  Verbindung, gegen den Testserver). Beide bleiben als Form-Zusicherung
  stehen; ihre Grenze steht im Bericht.
- **Die Kombination „drei Hops × Schlüsselbund beim Start weg" ist nicht
  getestet.** Der Codepfad ist identisch zu den getesteten (`if
  !is_available()` an einer Stelle, Hop-Benennung an einer anderen); T12 fährt
  drei Hops, T5 fährt beide Zustände. Ein weiterer Test hätte dieselben zwei
  Zeilen noch einmal durchlaufen.
- **Die Hop-Angabe erreicht die Oberfläche nicht.** Sobald der Code bekannt
  ist, ersetzt das Frontend die ganze `message` durch den Locale-Text — so
  arbeitet `translateErrorCode` seit Spec 0024, und genau das verlangt A5/A6
  hier. Die Information aus Spec 0076 A-8 bleibt im DTO und im Log, aber
  nicht in der angezeigten Zeile. **Sie sichtbar zu machen wäre neuer
  UI-Text und damit eine Produktentscheidung**, die Spec 0098 nicht trifft:
  eigener Platzhalter im Locale-Text oder ein zweiter kleiner Textteil
  darunter. Nicht in diesem Schritt entschieden, als Vorschlag im Bericht.
- **`KEYCHAIN_ACCESS_FAILED` fehlt in `FIVE_MINUTE_PATH_ERROR_CODES`.**
  `KEYCHAIN_UNAVAILABLE` fehlt dort ebenfalls; die beiden sind damit
  konsistent behandelt, und A6 verlangt nur `KNOWN_ERROR_CODES`. Beide
  aufzunehmen ist eine eigene, kleine Entscheidung über den Umfang dieser
  Liste und gehört nicht in diesen Schritt.
- **Ein offener Schlüsselbund-Dialog kann den Verbindungs-Timeout
  auslösen.** `resolve_auth` läuft je Hop innerhalb des Zehn-Sekunden-
  Fensters; wartet der Nutzer länger am Systemdialog, meldet die App
  `SSH_TIMEOUT` statt des Schlüsselbund-Codes. Kein A4-Verstoß — es entsteht
  gar kein `Backend`-Fehler —, aber der realistischste Weg, auf dem der
  Nutzer die neue Meldung nicht sieht. Ob das Lesen der Secrets vor das
  Timeout-Fenster gehört, ist eine Produktentscheidung außerhalb dieser Spec.

## 6. Weiterzugeben

`SshError` hat eine neue Variante. Exhaustive `match`-Stellen im Pro-Code
können dadurch nicht mehr übersetzen. In diesem Repo ist alles grün; beim
nächsten Submodule-Bump ist es zu prüfen.
