# Spec: Windows-Politur für die Testphase

Status: Entwurf
Repo: **öffentlich** `smart_ssh` (Kern-Verhalten + Titelleiste) — prüfen,
ob Teile privat sind (Lizenz-Eingabe-Feld)
Abhängigkeiten: Credential-Handling (0022), KI-Provider (0006), i18n/Logging
(0016/0024), custom Titelleiste (0014), Multi-Tab/Drag (0017)

> Vier Windows-spezifische Funde aus dem ersten echten Windows-Test. Ziel:
> Windows von 🟡 auf 🟢 (tester-bereit) bringen. macOS/Linux dürfen durch
> keine Änderung brechen — die Fixes sind entweder plattformneutral (Trim)
> oder plattform-**ergänzend** (Windows-Fensterbuttons zusätzlich zu macOS).
> **Priorität ERHÖHT für Fund 1** (Credential-Handling), sonst NORMAL.

## Fund 1 — Whitespace/CRLF in eingefügten Credentials trimmen (BLOCKER, ERHÖHT)

Beim **Einfügen** eines API-Keys auf Windows kommt ein `\r\n`/Whitespace mit,
das ungetrimmt an den Provider gesendet wird → Auth scheitert ("Credentials
ungültig"). Bestätigt mit Anthropic; betrifft jeden Einfüge-Vorgang.

**Fix**: **Alle** Credential-/Schlüssel-Eingaben werden beim Speichern
getrimmt (führende/nachfolgende Whitespaces inkl. `\r`, `\n`, Tabs):
- KI-Provider-**API-Key**.
- **Passwort** und **Sudo-Passwort** eines Servers.
- **Key-Passphrase**, falls per Feld eingegeben.
- **Lizenzschlüssel** (wird ebenfalls per Copy-Paste eingetragen — dort ggf.
  schon getrimmt, prüfen; sonst ergänzen).
- Ggf. **Base-URL**/Endpunkt-Felder (ein mitkopiertes Leerzeichen/Newline
  bricht auch die).

Umsetzung an **einer zentralen, gut testbaren Stelle** pro Eingabeart (nicht
über die UI verstreut) — das Trimmen gehört in die Speicher-/Validierungs-
logik im Backend, damit es unabhängig vom Eingabeweg (Paste, Tippen, später
Import) greift. Test: ein Key mit angehängtem `\r\n` wird korrekt getrimmt
gespeichert; ein Key mit *innen*liegenden Zeichen bleibt unverändert (nur
Rand trimmen, nicht mittendrin).

**Vorsicht — nicht zu viel trimmen**: Nur führende/nachfolgende Whitespaces.
Ein Passwort, das absichtlich mit einem Leerzeichen *endet*, ist ein
theoretischer Sonderfall — aber Whitespace-am-Rand in Zugangsdaten ist so
selten und CRLF-beim-Einfügen so häufig, dass Rand-Trimmen die klar richtige
Abwägung ist. Im Zweifel dokumentieren (ADR), aber trimmen.

## Fund 2 — Provider-Fehlerantwort loggen

Aktuell wird der ausgehende Request geloggt (`outgoing session context to AI
provider`), aber **nicht die Fehlerantwort** des Providers. Bei der Diagnose
musste geraten werden.

**Fix**: Bei einem Provider-Fehler (Auth, Rate-Limit, Netzwerk, Modell nicht
gefunden) wird die Antwort geloggt — **Status + Fehlertyp/-nachricht des
Providers**, aber **redigiert** (niemals der API-Key oder andere Secrets im
Log — Redaction-Invariante gilt auch hier). So sieht man bei einem Tester-
Problem den echten Grund (z. B. Anthropic 401 "invalid x-api-key") statt nur
"Credentials ungültig". Ergänzt die verständliche UI-Meldung (Spec 0047 D2),
ersetzt sie nicht.

## Fund 3 — Fenster-Steuerung auf Windows (Minimieren/Maximieren/Schließen)

Die custom Titelleiste (Spec 0014) wurde für macOS gebaut (Ampel-Buttons
links). Auf Windows erscheint **nur ein Schließen-Button** — kein
Minimieren/Maximieren. Für Windows-Nutzer wirkt das kaputt (Grundfunktion).

**Fix**: Die Titelleiste rendert **plattformspezifisch** die passenden
Fenster-Steuerelemente:
- macOS: wie bisher (Ampel links) — **unverändert**.
- Windows: Minimieren / Maximieren(Wiederherstellen) / Schließen **rechts**,
  in Windows-typischer Anordnung und Verhalten. Die Buttons müssen die
  tatsächlichen Tauri-Fenster-Kommandos auslösen (`minimize`, `maximize`/
  `unmaximize`, `close`).
- (Linux: prüfen, ob dort dasselbe Problem besteht — GNOME/KDE erwarten
  meist rechts. Falls ja, mitlösen; falls die Standard-Dekoration dort
  greift, so lassen.)

## Fund 4 — Fenster-Ziehen (Drag) auf Windows

Das Ziehen des Fensters an der Titelleiste (`data-tauri-drag-region`, macOS
via Spec 0017 gefixt) funktioniert auf Windows nicht. Wahrscheinlich
dieselbe Wurzel wie Fund 3 (custom Titelleiste plattformspezifisch
unvollständig).

**Fix**: Sicherstellen, dass die Drag-Region der Titelleiste auf Windows
korrekt greift (Fenster lässt sich an der Leiste ziehen), **ohne** dass die
neuen Fenster-Buttons (Fund 3) oder andere interaktive Elemente die Drag-
Region schlucken bzw. umgekehrt (dieselbe Drag-vs-Klick-Abgrenzung wie beim
macOS-Fix aus Spec 0017 — ein Button darf kein Drag auslösen, die Leiste
dazwischen schon). Zusammen mit Fund 3 lösen, da gemeinsame Ursache
wahrscheinlich.

## Sicherheits-/Konsistenz-Invarianten

- Fund 1: Nur Rand-Whitespace trimmen, Credential-Inhalt nie verändern.
- Fund 2: **Kein Secret** (API-Key etc.) landet im Provider-Fehler-Log.
- Fund 3/4: macOS-Titelleiste bleibt unverändert; die Windows-Ergänzung
  bricht keine bestehende Funktionalität.

## Testbarkeit

- Fund 1: Trim-Test je Eingabeart (Rand-`\r\n` weg, Innen-Zeichen bleiben);
  Regressionstest gegen den ungetrimmten Stand (Auth scheitert vorher).
- Fund 2: Ein simulierter Provider-401 landet redigiert im Log (Key nicht).
- Fund 3/4: schwer automatisiert (natives Fenster) — Komponententest für die
  Button-Präsenz/Drag-Attribute je Plattform, plus manuelle Windows-
  Verifikation durch Stefan.

## Offene Zuordnung

Prüfe, ob die Lizenz-Eingabe (Fund 1, Lizenzschlüssel-Trim) im **privaten**
Repo liegt — falls ja, ist das ein Zwei-Repo-Vorgang: der Trim-Mechanismus im
öffentlichen Kern, die Lizenz-Feld-Nutzung privat. Melde das, statt es zu
raten.
