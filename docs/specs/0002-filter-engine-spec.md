# Spec: Filter-/Policy-Engine

Status: Entwurf
Modul: `crates/core/filter`
Abhängigkeiten: keine (reine Logik, kein SSH, keine KI-API, kein UI)

## 1. Ziel

Jedes Kommando, das die KI vorschlägt (oder das automatisch aus einem KI-Workflow
entsteht), muss durch diese Engine, bevor es an eine SSH-Session geht. Die Engine
entscheidet: **AutoExec**, **Confirm** (Nutzer muss bestätigen) oder **Deny**
(wird gar nicht erst zur Bestätigung angeboten, z. B. weil syntaktisch nicht
zerlegbar/unsicher).

Kernprinzip: **Fail-safe defaults.** Alles, was nicht eindeutig auf der Whitelist
steht, landet mindestens bei `Confirm`. Nichts landet automatisch bei `AutoExec`,
außer es matcht explizit eine Regel.

## 2. Kernkonzepte

```rust
pub enum Decision {
    AutoExec,
    Confirm { reason: String },
    Deny { reason: String },
}

pub enum RuleAction {
    Allow,   // macht Kommando AutoExec-fähig
    Confirm, // erzwingt Bestätigung, auch wenn andere Regel Allow sagt
    Deny,    // blockt komplett, keine Ausführung möglich
}

pub struct Rule {
    pub id: String,
    pub pattern: Pattern,       // s.u.
    pub action: RuleAction,
    pub scope: Scope,           // s.u.
    pub priority: i32,          // höher = wird zuerst geprüft
}

pub enum Pattern {
    Glob(String),     // z.B. "ls *", "cat /var/log/*"
    Regex(String),    // für komplexere Fälle
    Exact(String),
}

pub enum Scope {
    Global,
    Server(ServerId),
    Tag(String),        // z.B. "production", "dev"
}
```

## 3. Präzedenz-Regeln (wichtigster Teil der Spec)

> **Erweitert durch Spec 0037 (Entitlements & Editionen)**: Die hier
> beschriebene Kette bekommt eine zusätzliche Sortierstufe `RuleOrigin`
> (`Builtin` → `Organization` → `User`), die **innerhalb desselben
> Aktions-Tiers** und **vor** der Scope-Spezifität greift. Die unten
> beschriebene Reihenfolge bleibt in ihrer Substanz unverändert gültig —
> die maßgebliche, vollständige Fassung des Sortier-Algorithmus steht in
> Spec 0037, Abschnitt 5.

Reihenfolge der Auswertung, **nicht verhandelbar**:

1. **Hard-Blacklist (fest im Core codiert, nicht vom Nutzer entfernbar)**
   Kommandos wie `rm -rf /`, `dd if=* of=/dev/*`, `mkfs*`, `:(){ :|:& };:`
   (Fork-Bomb), direkte Manipulation von `/etc/shadow`, `shutdown`/`reboot`
   ohne explizite Nutzer-Freigabe. Diese Liste ergibt immer mindestens
   `Confirm`, nie `AutoExec` — unabhängig von Nutzerregeln.
2. **Nutzerdefinierte `Deny`-Regeln** (nach `Scope`-Spezifität sortiert:
   Server > Tag > Global, dann nach `priority`)
3. **Nutzerdefinierte `Confirm`-Regeln** (gleiche Sortierung)
4. **Nutzerdefinierte `Allow`-Regeln** (gleiche Sortierung)
5. **Default**, falls nichts matcht: `Confirm { reason: "keine Regel gefunden" }`

Das heißt: **Deny schlägt immer Confirm schlägt immer Allow.** Eine explizite
Allow-Regel für `systemctl *` wird von einer Deny-Regel für
`systemctl stop nginx` auf Server-Scope überstimmt.

## 4. Command-Parsing & Chaining-Schutz

Das ist der sicherheitskritischste Teil. Ein KI-Vorschlag wie

```
ls -la && rm -rf /var/backup
```

darf **nicht** als Ganzes gegen `ls *` gematcht werden. Vorgehen:

1. Kommando wird mit einem Shell-Lexer (empfehlenswert: `shell-words` oder
   eigener minimaler Parser) in Teilkommandos zerlegt, getrennt durch:
   `&&`, `||`, `;`, `|`, sowie Command-Substitution `$(...)` und Backticks.
   **Ebenfalls modelliert werden schreibende Output-Redirections** (`>`,
   `>>`, `2>`, `&>`): Ein Redirect-Ziel ist ein Schreibzugriff auf eine
   Datei und muss als solcher gegen die Policy geprüft werden — `ls -la >
   /etc/passwd` darf **nicht** unter einer `Allow: ls *`-Regel automatisch
   ausgeführt werden, nur weil das Kommando mit `ls` beginnt. Das
   Redirect-Ziel wird als eigener zu prüfender Bestandteil behandelt
   (mindestens `Confirm`, analog zu einem schreibenden Zugriff), nicht als
   bloßes Argument von `ls`. **Input-Redirection (`<`) wird bewusst nicht
   gesondert behandelt**: Ein Lesezugriff wie `cat < /etc/shadow` erhält
   ohnehin dieselbe Entscheidung wie `cat /etc/shadow`, da das Kommando
   selbst schon auf die Datei zugreift — eine Extra-Prüfung des
   `<`-Ziels brächte keine zusätzliche Sicherheit.
2. **Jedes Teilkommando wird einzeln durch die komplette Präzedenz-Kette
   (Abschnitt 3) geprüft.**
3. Die Gesamt-Decision ist das **strengste** Ergebnis aller Teile:
   `Deny > Confirm > AutoExec`. Ein einziges `Deny`-Teilkommando disqualifiziert
   den gesamten Befehl.
4. Falls der Parser das Kommando nicht sicher zerlegen kann (z. B.
   verschachtelte Quotes, ungewöhnliche Escape-Sequenzen, Here-Docs) →
   automatisch `Confirm` mit Grund "Kommando konnte nicht sicher analysiert
   werden", **nie** `AutoExec`.
5. Command-Substitution (`$(...)`, Backticks) wird grundsätzlich als eigener
   Subcheck behandelt und erzwingt mindestens `Confirm`, da sie dynamisch zur
   Laufzeit anderen Code ausführen kann, der zum Zeitpunkt der Prüfung nicht
   vollständig bekannt ist.
6. `sudo`/`doas`-Präfixe werden vor dem Matching entfernt und **zusätzlich**
   separat vermerkt (`elevated: true` im Decision-Kontext), sodass später z. B.
   eine Regel "alles mit sudo → immer Confirm" möglich ist.

   **Wichtig — vollständige Normalisierung, nicht nur ein Token**: Das
   Entfernen darf nicht bei einem einzelnen `sudo`/`doas` haltmachen. Ein
   Angreifer kann beliebig wrappen: `env rm -rf /`, `sudo -u root rm -rf /`,
   `sudo sudo rm -rf /`, `timeout 5 rm -rf /`, `xargs rm`, `chroot`, bare
   `VAR=value`-Präfixe, oder `sudo bash -c "rm -rf /"`. Die Normalisierung
   läuft deshalb als **Fixpunkt-Schleife**: bekannte Wrapper (Elevation,
   `env`, `timeout`, `xargs`, `chroot`, Variablen-Zuweisungen) werden
   wiederholt abgeschält, bis sich der Kommandokopf nicht mehr ändert —
   erst dann wird gematcht. Das effektive, normalisierte Kommando ist die
   Basis für **jede** Prüfung (Hard-Blacklist, Nutzerregeln, Risiko-
   Klassifizierer), damit keine dieser Ebenen eine schwächere Sicht auf das
   Kommando hat als die anderen. Auch `bash -c "..."`/`sh -c "..."` wird als
   Wrapper behandelt: Der Inhalt des `-c`-Arguments wird als eigenes
   Kommando (bzw. bei Nicht-Parsebarkeit als `Confirm`-pflichtig) geprüft,
   nicht als undurchsichtiges Argument durchgewunken.

7. **Längenbegrenzung als Absturzschutz**: Vor dem Parsen wird die
   Kommandolänge geprüft (Cap z. B. 4096 Zeichen). Auch die Rekursionstiefe
   bei verschachtelter Command-Substitution (`$(echo $(echo ...))`) wird
   begrenzt. Überschreitung → `Confirm` mit entsprechendem Grund, **nie** ein
   unbegrenzter rekursiver Abstieg, der den Prozess per Stack-Overflow
   abbrechen lassen könnte. Diese Grenze gilt für **jeden** Konsumenten der
   Parselogik gleichermaßen (Filter-Engine wie Risiko-Klassifizierer) —
   kein Konsument parst ungebremst, wo ein anderer bremst.

## 5. Öffentliche Schnittstelle

```rust
pub trait PolicyStore {
    fn rules_for(&self, scope: &EffectiveScope) -> Vec<Rule>;
}

pub struct FilterEngine<S: PolicyStore> {
    store: S,
}

impl<S: PolicyStore> FilterEngine<S> {
    pub fn evaluate(&self, command: &str, ctx: &EvalContext) -> Decision;
}

pub struct EvalContext {
    pub server_id: ServerId,
    pub tags: Vec<String>,
}
```

`PolicyStore` ist ein Trait, damit Tests eine In-Memory-Implementierung nutzen
können, ohne die spätere DB-Anbindung zu brauchen.

## 6. Testfall-Katalog (Auszug — vollständige Suite folgt als eigene Datei)

| # | Input | Erwartung | Grund |
|---|-------|-----------|-------|
| 1 | `ls -la` (Whitelist: `ls *`) | AutoExec | einfacher Whitelist-Treffer |
| 2 | `rm -rf /` | Confirm (mind.) | Hard-Blacklist greift immer |
| 3 | `ls -la && rm -rf /var/backup` | Deny/Confirm (strengstes Teilergebnis) | Chaining darf Blacklist nicht umgehen |
| 4 | `ls $(cat /etc/passwd)` | Confirm | Command-Substitution erzwingt Confirm |
| 5 | `systemctl status nginx` (Tag "production": Deny für `systemctl *`) | Deny | Scope-Präzedenz Server/Tag > Global |
| 6 | `echo "ls -la"` | wie konfiguriert für `echo *` | kein automatisches "Inhalt von echo interpretieren" |
| 7 | `sudo apt update` | Confirm, falls Regel "sudo → Confirm" aktiv | Elevated-Flag |
| 8 | Kommando mit verschachtelten/unklaren Quotes | Confirm | Parser-Fallback, nie AutoExec |
| 9 | `ls -la; rm important.txt` (kein Whitelist-Treffer für `rm`) | Confirm | Default-Fallback für unbekannten Teil |
| 10 | Leerer/nur-Whitespace-String | Deny | kein sinnvolles Kommando |
| 11 | Kommando > konfigurierbares Längenlimit | Confirm | Schutz vor Obfuskierung durch extrem lange Payloads |
| 12 | Zwei widersprüchliche Nutzerregeln gleicher Priorität (Allow vs Confirm, gleicher Scope) | Confirm | im Zweifel die strengere Regel |

## 7. Offene Punkte für Diskussion

- Soll es eine **Simulationsansicht** geben ("teste diese Regel gegen
  Beispielkommandos"), bevor eine Regel aktiv gesetzt wird?
- Sollen Regeln **zeitlich befristet** werden können (z. B. "für die nächste
  Stunde `systemctl restart *` erlauben")?
- Wie gehen wir mit **mehrzeiligen Skripten** (Heredoc, `bash -c "..."` mit
  komplexem Inhalt) um — komplett verbieten (immer Confirm als Ganzblock,
  kein Sub-Parsing) oder versuchen zu parsen? Empfehlung: fürs MVP komplett
  als ein Block behandeln und immer `Confirm` erzwingen, kein Sub-Parsing —
  deutlich geringere Fehleranfälligkeit als ein unvollständiger Parser.
