# ADR 0094 — Das MCP-Token in der Datenbank, und was dabei offen bleibt

Status: angenommen
Bezug: Spec 0101 (A12, T12, T1), Spec 0028 (§9), ADR 0093

## Zusammenhang

Spec 0101 A12 verlangt, dass das MCP-Server-Token in der verschlüsselten
Datenbank liegt statt im Klartext in `settings.json`. Bei der Umsetzung
waren mehrere Punkte zu entscheiden, die die Spec offen lässt, und zwei
Funde aus dem Review bleiben bewusst stehen.

## Entscheidungen

### 1. Der Name des Platzes steht in `core`, nicht in `app-logic`

`MCP_SERVER_TOKEN_REF` (`mcp:server-token`) brauchen zwei Schichten:
`app-logic` zum Lesen und Schreiben und die Rohdatei-Prüfung aus T1 in
`persistence-sqlite`, die nicht auf `app-logic` zeigen darf. Ein zweites
Literal könnte abdriften, ohne dass irgendwo etwas scheitert.

Ein eigener Namensraum (`mcp:`), nicht `server:` oder `ai-provider:`: Das
Token gehört zu keinem Server und zu keinem Provider. Es fällt damit aus
der Positivliste des Secret-Umzugs (A10) — richtig so, denn es hat nie im
Schlüsselbund gelegen.

### 2. Die Reihenfolge des Umzugs liegt in `app-logic`

`settings.json` ist nur über `tauri-plugin-store` erreichbar, also über
Tauri. Damit T12 ohne Fenster läuft, steckt die Reihenfolge (schreiben,
zurücklesen, vergleichen, dann entfernen) in `app_logic::mcp_token`; der
alte Ablageort steht hinter dem Trait `LegacyMcpTokenFile`. In `app-shell`
bleibt nur der Dateizugriff — und zwar nur noch lesend und löschend. Dass
dorthin nicht mehr geschrieben werden *kann*, ist damit strukturell, nicht
bloß eine Zusicherung im Text.

### 3. Ein leeres Token wird nicht übernommen

A12 sagt „mit gleichem Wert übernommen". Für `"mcpServerToken": ""` wäre
das falsch: Ein leeres erwartetes Token heißt MCP ohne Geheimnis. Der
Umzug erzeugt in diesem Fall ein richtiges Token und entfernt den leeren
Eintrag. Das weicht vom Wortlaut ab und ist eine Verschärfung.

### 4. Ein gescheitertes Entfernen hält den Start nicht auf — mit einer Ausnahme

Bleibt nach erfolgreichem Umzug eine Kopie in `settings.json` liegen, geht
eine Warnung ins Log und jeder folgende Aufruf versucht es erneut
(dasselbe Muster wie A11 für Schlüsselbund-Löschungen). **Im Umzugszweig
selbst** scheitert das Entfernen dagegen hart: Dort ist die Klartext-Kopie
gerade erst überflüssig geworden, und ein stiller Weiterlauf verschwiege
genau den Zustand, den A12 beseitigen soll. Der Reviewer hält die
Ungleichheit für einen kleinen Mangel; sie ist Absicht und hier
festgehalten.

## Bewusst nicht behoben

### Das Token steht weiterhin im DTO

Spec 0101 §6 verlangt „kein Token in … DTO"; Spec 0028 §9 verlangt, dass
das Token in den Einstellungen **immer angezeigt** wird. Beides zugleich
geht nicht. Umgesetzt ist Spec 0028 (`McpServerSettingsDto.token`), weil
der Nutzer das Token zum Eintragen in seinen Client braucht und es ohne
Anzeige unbrauchbar wäre. Der Widerspruch ist eine Produktentscheidung und
liegt beim Menschen: entweder §6 präzisieren („nicht in persistierten
Daten, nicht in Log und Diagnosepaket") oder die Anzeige ändern.

### Kein Geländer gegen ein leeres Token in der MCP-Middleware

`crates/mcp-server/src/auth.rs` vergleicht das mitgelieferte Bearer-Token
mit dem erwarteten, ohne zu prüfen, ob das erwartete leer ist. Mit
Entscheidung 3 kann ein leeres Token auf dem A12-Weg nicht mehr entstehen;
der Weg dorthin ist aber nicht der einzige denkbare. Ein `if expected
.is_empty() { Unauthorized }` wäre billig — es schlösse jedoch jeden aus,
der heute mit einem leeren Token arbeitet, und ist damit eine
Verhaltensänderung außerhalb dieser Spec. Entscheidung liegt beim
Menschen.

### Der tatsächliche `settings.json`-Zugriff ist ungetestet

`SettingsJsonToken` ist gegen einen Doppelgänger geprüft, nicht gegen
`tauri-plugin-store`. Die Fehlersemantik des echten Stores weicht ab —
genau daraus entstand der Fund „gescheitertes Schreiben lässt das Token in
der Datei stehen". Behoben ist der Fund; der Nachweis am echten Store
fehlt weiterhin. Dafür steht im selben Modul ein Harness bereit
(`first_run_notice::test_support::test_app`).

### `settings.json` wird von vier weiteren Modulen ohne Härtung geschrieben

`first_run_notice`, `chat_retention`, `local_server` und
`risk_second_opinion` schreiben die Datei mit Umask-Rechten. Seit diesem
Schritt härtet jeder Schreib- **und** Leseweg dieses Moduls sie wieder auf
0600, das Loch bleibt aber an den anderen vier Stellen. Eine zentrale
Hilfsfunktion für jeden `save()` gehört in einen eigenen Schritt.
