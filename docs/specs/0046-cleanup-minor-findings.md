# Spec: Aufräum-Paket kleinerer Funde (öffentliches Repo)

Status: Entwurf
Modul: `crates/app-shell`, `crates/persistence-sqlite`, `frontend/` (mehrere
Stellen) — alles öffentliches Repo `smart_ssh`
Abhängigkeiten: Server-Verwaltung (0008), Regel-Verwaltung (0009), Notiz-/
Diff-Anzeige (0019), Multi-Tab (0017), lokaler Pseudo-Server (0032)

> Sammel-Spec für mehrere kleine, unabhängige Funde aus dem großen Audit und
> eigener Beobachtung. Jeder Fund ist ein eigener committbarer Fix — sie sind
> nur gebündelt, weil sie alle klein, öffentlich und risikoarm sind. **Ein
> Fund (Fund 1, `delete_server`) berührt Credential-Handling und bekommt
> erhöhte Review-Priorität**, der Rest ist NORMAL.

## Fund 1 — `delete_server` ohne Bestätigung/Vorschau (erhöhte Sorgfalt)

`delete_server` (Spec 0008) löscht einen Server **ohne Bestätigung oder
Vorschau** — obwohl es unwiderruflich Keychain-Secrets löscht **und**
Jump-Host-Referenzen anderer Server still auf `NULL` setzt. Das steht im
Kontrast zum zweistufigen Löschen einer Gruppe (Spec 0008, Abschnitt 3), das
eine Cascade-Vorschau zeigt.

**Fix**:
- `delete_server` bekommt (analog zu `delete_group`) eine Vorschau, die vor
  dem tatsächlichen Löschen zeigt: welche Secrets aus dem Keychain entfernt
  werden und **welche anderen Server ihre Jump-Host-Referenz auf diesen
  Server verlieren** (die still auf `NULL` gesetzt würden). Erst ein zweiter,
  bestätigender Aufruf löscht.
- UI: Bestätigungsdialog, der die Vorschau anzeigt.
- Reihenfolge beim Löschen bleibt wie gehabt (erst Keychain, dann DB, Spec
  0008), aber erst nach Bestätigung.

**Review-Priorität ERHÖHT** für diesen Fund (Credential-Handling,
irreversibel).

## Fund 2 — Regel-Test-Panel leitet Tags nicht vom gewählten Server ab

Das Regel-Test-/Simulationspanel (Spec 0009, Abschnitt 6) leitet die Tags
**nicht** vom gewählten Server ab. Folge: Es kann "keine Regel matcht"
anzeigen für ein Kommando, das in Wirklichkeit über eine tag-scoped Regel
`AutoExec` ausführen würde. Das ist ein transparenz-untergrabender Bug in
genau dem Panel, das Vertrauen schaffen soll — der Nutzer prüft eine Regel
und bekommt ein falsches Ergebnis.

**Fix**: Wählt der Nutzer im Test-Panel einen Server als Simulations-Scope,
werden dessen Tags (aus `server_tags`, Spec 0004) automatisch in den
`EvalContext` übernommen, sodass tag-scoped Regeln korrekt greifen — das
Panel zeigt dieselbe Entscheidung, die im echten Betrieb fiele. Optional
weiterhin manuell zusätzliche Tags simulierbar.

## Fund 3 — Diff-Vorschau großer Remote-Dateien ohne Größen-Cap

Die Diff-Vorschau (Notiz-Diff Spec 0019, und der Datei-Write-Diff aus Spec
0020) hat **keinen Größen-Cap** — anders als der Lesepfad (256-KB-Cap, Spec
0020). Eine sehr große Datei/ein sehr großer Inhalt könnte den
Confirm-Dialog-Renderer einfrieren (der Zeilen-Diff ist O(n·m), gedacht für
kurzen Notiztext, nicht für riesige Dateien).

**Fix**: Größen-Cap für die Diff-Berechnung (z. B. dieselbe 256-KB-Grenze
wie der Lesepfad, oder ein eigener sinnvoller Wert). Überschreitet der
alte oder neue Inhalt den Cap: **kein** zeilenweiser Diff berechnen,
stattdessen ein Hinweis ("Inhalt zu groß für Zeilen-Diff") plus die reine
Information alt/neu-Größe — analog zum Binärdatei-Fall aus Spec 0020,
Abschnitt 4.2. Der Schreibvorgang selbst bleibt möglich (nur die Vorschau
ist gekürzt), die Bestätigung erfolgt dann ohne detaillierten Diff.

## Fund 4 — Tab schließen mit Reload dazwischen verweigert wartende Aktion nicht

"Tab schließen = wartende Aktion ablehnen" (Spec 0017, Abschnitt 5) hängt am
**Frontend-State**, den ein Reload (Dev-Hot-Reload, oder ein
Frontend-Neustart) auf `NULL` zurücksetzt. Folge: Nach einem Reload weiß das
Frontend nichts mehr von der wartenden Aktion, schließt den Tab ohne die
Ablehnung auszulösen — und das **Backend wartet dann ewig** auf eine
Entscheidung, die nie kommt (der `oneshot`-Channel aus Spec 0007 wird nie
aufgelöst).

**Fix**: Die Auflösung einer wartenden Aktion darf nicht allein vom
Frontend-State abhängen. Zwei mögliche Wege (Coder wählt den saubereren):
- Backend-seitiges Timeout für wartende `Confirm`-Aktionen (analog zum
  MCP-Timeout aus Spec 0028) — läuft es ab, gilt die Aktion als abgelehnt,
  der Channel wird aufgelöst, kein ewiges Warten.
- Oder: Beim Frontend-(Re)start werden über `list_sessions` (Spec 0017) die
  wartenden Aktionen rekonstruiert, sodass der Schließen-Handler sie
  weiterhin kennt.
Empfehlung: das Backend-Timeout als robuste Grundsicherung (verhindert
ewiges Warten unabhängig von der Frontend-Ursache), die State-Rekonstruktion
optional obendrauf. Der Coder beschreibt seine Wahl.

## Fund 5 — Lokaler Pseudo-Server zeigt Port 0

Der lokale Pseudo-Server (Localhost, Spec 0032) zeigt in der UI einen Port
`0` an. Er hat konzeptionell **keinen** Port (läuft über direkte
Prozessausführung, nicht SSH/TCP).

**Fix**: Für `is_local`-Server wird die Port-Anzeige **ausgeblendet** (nicht
`0` gezeigt) — überall, wo Server-Metadaten dargestellt werden (Liste,
Detailansicht, Tab). Reiner Anzeige-Fix.

## Testbarkeit

- Fund 1: `delete_server`-Vorschau listet korrekt betroffene Jump-Host-
  Referenzen und Keychain-Secrets; tatsächliches Löschen erst nach
  Bestätigung; Regressionstest, dass ohne Bestätigung nichts gelöscht wird.
- Fund 2: Test-Panel mit gewähltem Server übernimmt dessen Tags; ein
  Kommando, das über eine tag-scoped Regel `AutoExec` wäre, zeigt das im
  Panel korrekt (nicht "keine Regel matcht").
- Fund 3: Inhalt über dem Cap → kein O(n·m)-Diff, Hinweis + Größenangabe;
  Inhalt unter dem Cap → normaler Diff.
- Fund 4: wartende Aktion + simulierter Reload/State-Verlust → Backend
  wartet nicht ewig (Timeout greift bzw. State wird rekonstruiert).
- Fund 5: `is_local`-Server zeigt keinen Port; normaler Server zeigt seinen
  Port unverändert.

## Sicherheits-Invarianten

- Fund 1: Das zweistufige Löschen ändert nichts an der bestehenden
  Lösch-Reihenfolge (erst Keychain, dann DB) — nur eine Bestätigung davor.
- Fund 4: Ein Backend-Timeout, das eine wartende Aktion auflöst, gilt als
  **Ablehnung** (fail-safe), nie als Genehmigung.
- Keiner der Fixes lockert eine Filter-Engine-Entscheidung oder ein Gating.

## Hinweis

Jeder Fund ist ein eigener Commit. Fund 1 (erhöhte Priorität) bekommt den
`spec-reviewer` mit ERHÖHT; für das Gesamtpaket reicht sonst NORMAL.
