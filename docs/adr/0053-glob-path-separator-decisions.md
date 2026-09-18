# 0053 — Glob `*` überquert `/` nicht mehr (Spec 0060): Erkennungs-Ansatz & Zusammenspiel

## Status

Angenommen

## Kontext

Spec 0060 verlangt, dass ein Glob-`*` in einem **pfadförmigen** Muster
keine `/`-Grenze mehr überquert (Filter-Engine-Umgehungs-Fix, Release-Gate
C). Die Spec verlangt ausdrücklich, vor der Umsetzung den
„pfadförmig"-Erkennungsansatz und das Zusammenspiel mit dem bestehenden
lexikalischen Pfad-Normalizer zu klären und zu berichten (§1/§2) — dieser
ADR hält beides fest, plus zwei während der Umsetzung gefundene, nicht
offensichtliche Entscheidungen (Scope-Eingrenzung auf die Filter-Engine;
Migrationshinweis für bestehende mehrstufige Pfad-Regeln).

## Entscheidungen

### 1. Erkennung „pfadförmig": jedes Token mit `/`, außer URL-Schema

Ein Muster (`"cat /var/log/*"`, komplette Kommandozeile inkl. Kommandoname)
wird whitespace-tokenisiert; es gilt als pfadförmig, wenn **mindestens ein
Token ein `/` enthält UND nicht `://` enthält**
(`crates/core/src/filter/pattern.rs::is_path_shaped_pattern`).

- Absolute Pfade (`/var/log/*`) sind der klare Fall.
- Explizit relative Pfade (`./foo/*`, `../foo/*`) fallen unter dieselbe
  Regel — dieselbe `/`-Überquerungs-Gefahr besteht dort identisch.
- **Bare** relative Pfade ohne führendes `./` (`foo/bar/*`) gelten
  ebenfalls als pfadförmig — „im Zweifel strenger" (Spec 0060 §1): jedes
  Token mit `/` sieht potenziell wie ein Dateipfad aus.
- **Ausnahme**: ein Token mit einem URL-Schema (`irgendwas://…`) gilt
  NICHT als pfadförmig, sonst bräche der von der Spec selbst genannte
  Normalfall (`curl http://example.com/*` muss weiterhin `*` über `/`
  matchen können).
- Enthält EIN Token im Muster ein `/` (keine URL), gilt das GESAMTE Muster
  als pfadförmig (`literal_separator` gilt für den ganzen kompilierten
  Glob — `globset` kennt keine gemischte Pro-Token-Konfiguration). Ein
  seltener gemischter Fall (`wget http://x/* -O /tmp/*`) würde dadurch
  auch im URL-Teil strenger — bewusst in Kauf genommen (Sicherheit vor
  Bequemlichkeit).

### 2. Zusammenspiel mit dem Pfad-Normalizer: erst normalisieren, dann matchen

Der bestehende SFTP-Traversal-Normalizer
(`app_shell::orchestration::normalize_remote_path`, Spec 0020) läuft
**einmal**, vor der Filter-Auswertung, aber **nur** für die beiden
SFTP-Pseudo-Kommandos — für diesen Fall ist `cmd` bei Ankunft in
`core::filter` bereits `.`/`..`-frei (idempotenter No-op, kein
Widerspruch). Für **jede andere** Shell-Kommandozeile (nie durch den
SFTP-Normalizer gelaufen) reicht `literal_separator` allein nicht:

```
/var/log/*  matcht  /var/log/..
```

— `..` enthält selbst kein `/`, ein einzelnes `*` akzeptiert es trotz
`literal_separator` als „ein Segment", was lexikalisch `/var` ist,
außerhalb des erlaubten Verzeichnisses. Deshalb normalisiert
`crates/core/src/filter/pattern.rs::normalize_path_shaped_tokens` jedes
pfadförmige Token in `cmd` **vor** dem Matching, mit derselben rein
lexikalischen `.`/`..`-Auflösung wie der bestehende Normalizer
(`normalize_lexical_path`, unabhängig dupliziert statt geteilt — `core`
darf laut Architektur-Regel nicht von `app-shell` abhängen, nur
umgekehrt).

### 3. Scope-Eingrenzung: nur die Filter-Engine, NICHT der Risiko-Klassifizierer

**spec-reviewer-relevanter Fund während der Umsetzung**: `Pattern` wird
von `crate::risk` für eigene, hart codierte Muster wiederverwendet (z. B.
`"*dd*if=**of=/dev/*"` für die Red-Risiko-Einstufung von
`dd ... of=/dev/...`). Eine erste Fassung wendete die neue Logik direkt in
`Pattern::matches` an — das brach empirisch
`test_server_risk_red_dd_to_device`: das `**` zwischen `if=` und `of=` ist
in `globset` nur dann grenzüberschreitend, wenn es eine EIGENSTÄNDIGE
Pfad-Komponente ist (von `/` flankiert); hier steht es mitten im Text ohne
umgebende `/` und verhält sich unter `literal_separator` wie ein
gewöhnlicher (nicht-kreuzender) Wildcard — die Red-Einstufung fiel auf
„kein Risiko" zurück. Das ist exakt die von CLAUDE.md verbotene Richtung
(„Escalation only goes one direction").

**Fix**: eine neue, separate Methode `Pattern::matches_for_user_rule`
trägt die Spec-0060-Logik, aufgerufen ausschließlich von
`crate::filter::engine::evaluate_rules_explained` (Nutzer-/Organisations-
Regeln, Allow **und** Deny). `Pattern::matches` selbst bleibt unverändert
und wird weiterhin vom Risiko-Klassifizierer (`crate::risk`) und der
Hard-Blacklist (`crate::filter::blacklist`) genutzt — beide haben aktuell
keine pfadförmigen Muster im o. g. Sinn (die Blacklist hat nur `"*mkfs*"`,
kein `/`), eine künftige Änderung dort müsste diese Abgrenzung erneut
prüfen.

### 4. Anwendung auf Allow UND Deny — mit Oder-Rückfall für Deny/Confirm

Die strengere Auswertung gilt für alle drei Text-Varianten
(original/stripped/resolved) und für **beide** Regel-Aktionen — aber mit
einer wichtigen Asymmetrie, die erst der spec-reviewer-Durchgang (s.
Abschnitt 6) aufdeckte: für `Deny`/`Confirm` verknüpft
`matches_for_user_rule` die neue, striktere Prüfung per ODER mit dem alten,
permissiven `Pattern::matches` (`*` überquert `/`, keine Normalisierung).
Ursprünglich war die Begründung „ein nicht mehr matchendes Deny fällt
ohnehin nur auf `Confirm` zurück, nie auf `AutoExec`, also sicher genug" —
das stimmt zwar (die Kette bleibt sicher), verletzt aber wörtlich CLAUDE.md
(„ein regelbasiertes Deny darf nie abgeschwächt werden") und hätte ein
bestehendes `Deny: rm /home/user/*` für einen völlig harmlosen,
NICHT-Ausbruch-Zugriff wie `rm /home/user/sub/file` stillschweigend von
„hart verboten" auf „Nutzer muss bestätigen" herabgestuft. Die
Oder-Verknüpfung schließt das: das alte Matching erkennt weiterhin JEDEN
Fall, den es vor Spec 0060 erkannt hätte (reiner Superset), die neue
Normalisierung kommt nur zusätzlich hinzu. Für `Allow` bleibt es bei der
rein strikten Prüfung (kein Rückfall) — dort ist „weniger matcht" per
Spec-Vorgabe explizit der gewollte, sichere Ausgang.

### 5. Migrations-Hinweis: `**` nur für Deny/Confirm, NICHT für Allow

**Bricht ein bestehender, legitimer Anwendungsfall (Spec 0060 §3)?** Ja,
einer: eine bestehende Regel mit einem einzelnen `*`, die bewusst oder
unbewusst mehrere Verzeichnisebenen abdecken sollte (z. B.
`Deny: sftp-write /etc/*` gedacht als „alles unter /etc verbieten",
matchte bisher auch `/etc/nginx/nginx.conf`). Nach dem Fix deckt ein
einzelnes `*` nur noch EINE Ebene ab. Ein bestehender Test
(`test_write_remote_file_deny_rule_blocks`,
`crates/app-shell/src/orchestration.rs`) kapselte genau diesen Fall und
wurde auf `/etc/**` umgestellt (empirisch verifiziert: `**` überquert `/`
auch mit `literal_separator = true`, solange es eine eigenständige
Pfad-Komponente ist — matcht `/etc/nginx.conf`, `/etc/nginx/nginx.conf`
und `/etc/a/b/c`, nicht aber `/var/x`).

**spec-reviewer-Fund (Abschnitt 6)**: `**` ist für `Allow`-Regeln NICHT
sicher als pauschaler Migrationsrat geeignet. Weil `Pattern::matches`
immer den GESAMTEN Kommandozeilen-String als eine einzige Zeichenkette
matcht (kein argv-bewusstes Parsing), matcht `**` auch über Leerzeichen
hinweg — `Allow: cat /tmp/**` erlaubt damit z. B. auch
`cat /tmp/a /etc/shadow` (ein zweites, völlig anderes Argument nach dem
eigentlich gemeinten Pfad), weil `**` „irgendwas, auch `/` und
Leerzeichen" bedeutet und dahinter kein Text mehr im Muster steht, der es
begrenzt. Ein einzelnes `*` hat dieses Problem nach dem Fix NICHT mehr
(es kann das zweite `/etc/shadow`-Argument nicht queren, weil das ein
eigenes `/` enthält) — das Problem betrifft ausschließlich die neu
eingeführte `**`-Fluchttür. Der CHANGELOG-Migrationsrat wurde entsprechend
differenziert: `**` nur für `Deny`/`Confirm` empfohlen (dort macht
zusätzliche Reichweite nie etwas unsicherer), für `Allow` stattdessen
mehrere spezifische, einstufige Allow-Regeln. Keine Code-Einschränkung für
`**` in Allow-Regeln eingebaut (argv-bewusstes Parsing wäre eine deutlich
größere, nicht in dieser Spec vorgesehene Änderung) — rein
dokumentarische Entschärfung, wie von der Spec für einen brechenden
Anwendungsfall verlangt.

### 6. spec-reviewer-Runde 2 (ERHÖHT + adversarial): gefunden und behoben

Der erste Implementierungsstand bestand den Build/Test-Gate, hatte aber
mehrere adversarial auffindbare Bypässe — der Pflicht-Review (s. CLAUDE.md)
fand sie vor Abschluss:

1. **KRITISCH — Shell-Quoting/-Escaping hebelte die `..`-Erkennung
   vollständig aus.** `normalize_lexical_path` vergleicht rohe
   Textsegmente gegen das Literal `".."` — `\..`, `".."`, `'..'`,
   `{..,..}`, `.[.]` sehen für diesen Vergleich alle anders aus als `..`,
   ergeben aber nach der Shell-Expansion auf dem Zielserver exakt `..`.
   Ein einziger Backslash reichte, um wieder `AutoExec` für einen
   `../`-Ausbruch zu bekommen. **Nicht vollständig lösbar**: eine
   vollständige lexikalische Nachbildung jeder Shell-Expansion ist
   unmöglich (Variablen-Substitution ist grundsätzlich nicht lexikalisch
   auflösbar, s. Fund 5 unten). **Fix**: „im Zweifel strenger" — enthält
   ein pfadförmiges Token in `cmd` klassische Shell-Metazeichen
   (`\ ' " { } [ ] $` `` ` ``), gilt die Normalisierung als nicht
   vertrauenswürdig; für `Allow` bedeutet das kein Match (sicher, fällt
   auf `Confirm` zurück), für `Deny`/`Confirm` bleibt der alte permissive
   Fallback (Fund/Abschnitt 4) trotzdem aktiv.
2. **`://`-Ausnahme auf der Kommando-Seite öffnete eine eigene Lücke.**
   `normalize_path_shaped_tokens` überspringt ursprünglich (wie die
   Muster-Klassifizierung) jedes Token mit `://` — auf der Kommando-Seite
   aber unbegründet: ein Token wie `/tmp/x://../../../etc/shadow` (ein
   zuvor im erlaubten Baum angelegtes Verzeichnis `x:` genügt) enthält
   zufällig `://` und wurde dadurch NICHT normalisiert, sodass die rohen
   `..`-Segmente unverändert gegen ein `**`-Muster durchgingen. **Fix**:
   die `://`-Ausnahme gilt jetzt NUR noch für die Muster-Klassifizierung
   (`is_path_shaped_pattern`, dort weiterhin nötig, damit
   `curl http://example.com/*` nicht bricht), nicht mehr für die
   Kommando-Token-Normalisierung — jedes Token mit `/` wird normalisiert,
   unabhängig vom Inhalt.
3. **`**` schluckt zusätzliche Argumente in Allow-Regeln** — s. Abschnitt
   5 oben (dokumentarisch entschärft, kein Code-Fix).
4. **Deny-Abschwächung ohne Rückfall** — s. Abschnitt 4 oben (Oder-
   Verknüpfung mit dem alten `Pattern::matches` für Deny/Confirm).
5. **Undokumentierte Restrisiken, jetzt hier festgehalten**:
   - **Symlinks**: die Normalisierung ist rein lexikalisch (wie der
     bestehende SFTP-Normalizer) und kann bei einem Symlink im erlaubten
     Verzeichnis falsch liegen — `cat /var/log/link/../shadow`
     normalisiert lexikalisch zu `/var/log/shadow` (kein Ausbruch), auch
     wenn `link` real auf `/etc` zeigt und der Kernel tatsächlich
     `/etc/shadow` liest. Kein Regress (vorher ebenso ungeschützt), aber
     explizit als Grenze dokumentiert statt nur implizit („keine
     Symlink-Auflösung" im Code-Kommentar).
   - **Variablen-/Kommando-Substitution** (`$X`, `` `x` ``, `$(x)`): ein
     Token ohne `/` (z. B. `$X`) gilt nicht als pfadförmig und wird nicht
     normalisiert; setzt eine vorherige Runde `X=../../etc/shadow`, sieht
     der Filter etwas anderes als die Shell tatsächlich ausführt. Nicht
     durch Spec 0060 verursacht (dieselbe Klasse besteht bereits ohne
     pfadförmige Muster), aber dieselbe grundsätzliche Grenze: der Filter
     bewertet Text, nicht Shell-Semantik.
   - Beide Punkte sind bewusst NICHT in dieser Runde behoben (kein
     AutoExec-Bypass — Symlinks können nur einen bereits erlaubten
     Zielbereich verlassen, wenn der Nutzer selbst zuvor einen
     entsprechenden Symlink angelegt hat; Variablen-Substitution
     erfordert eine vorherige, bereits vom Nutzer bestätigte Runde), aber
     als bekannte Grenze für eine künftige Spec festgehalten.
6. **Relative/Nachlaufslash-Muster-Asymmetrie** (kein Sicherheitsproblem,
   aber ein echter Funktionsbruch): nur `cmd`, nicht das MUSTER selbst zu
   normalisieren, ließ ein explizit relatives Muster wie `./foo/*`
   `./foo/x` nicht mehr matchen (Muster blieb `./foo/*`, `cmd` normalisierte
   zu `foo/x`) und behandelte einen Muster-Nachlaufslash (`/var/log/*/`)
   uneinheitlich zu `cmd`s entferntem Nachlaufslash. **Fix**: dieselbe
   `normalize_path_shaped_tokens`-Normalisierung läuft jetzt auf BEIDEN
   Seiten (Muster und `cmd`) — `*`/`**` sind für `normalize_lexical_path`
   nur opake Segmente (weder `""`, `"."` noch `".."`), bleiben also
   erhalten.
7. **Commit-Hygiene, nicht behoben**: der Feature-Commit für Spec 0060
   enthielt eine sachfremde `tracing::debug!`-Zeile in
   `crates/app-shell/src/orchestration.rs` aus einer früheren, separaten
   Live-Diagnose-Sitzung ("KI antwortet nicht") — kein Sicherheitsproblem
   (kein Secret im Log, die Zeile ist funktional korrekt und aktiv
   genutzt), aber sie hätte in einen separaten Commit gehört. Bewusst
   NICHT per Git-History-Rewrite korrigiert (lokale, bereits
   möglicherweise weitergegebene Commits, CLAUDE.md rät von unnötigem
   Umschreiben ab) — hier nur dokumentiert.

## Konsequenzen

- Der Risiko-Klassifizierer (`crate::risk`) bleibt vollständig unberührt —
  künftige pfadförmige Risiko-Muster müssten diese Abgrenzung explizit neu
  bewerten, statt sich auf `Pattern::matches_for_user_rule` zu verlassen.
- Bestehende Allow-/Deny-Regeln mit einem einzelnen `*` über mehr als eine
  Verzeichnisebene verlieren stillschweigend ihre bisherige Reichweite
  (fallen auf Confirm/Default zurück, nie auf AutoExec) — Nutzer mit
  solchen Regeln sollten sie nach diesem Update auf `**` prüfen (nur für
  Deny/Confirm sicher, s. Abschnitt 5).
- `is_path_shaped_pattern`/`normalize_path_shaped_tokens`/
  `normalize_lexical_path` sind unabhängige, für die Filter-Engine
  eigenständig getestete Kopien der SFTP-Normalizer-Logik — eine künftige
  Änderung an einer Seite (App-Shell-SFTP-Normalizer vs.
  Filter-Engine-Normalizer) muss beide Stellen im Blick behalten, bis eine
  gemeinsame Crate-Grenze das vereinheitlicht (nicht Teil dieser Spec).
- Symlink- und Variablen-Substitutions-Restrisiken (Abschnitt 6, Fund 5)
  bleiben bestehen — eine künftige Spec müsste sie explizit aufgreifen,
  falls sie sich als praktisch relevant erweisen.
