# 0036-substitution-depth-cap-denies-instead-of-confirms

## Status
Akzeptiert

## Kontext

Spec 0043, Abschnitt 3 (Fund B) schreibt für ein Kommando, dessen
Command-Substitutions-Verschachtelung den expliziten Rekursions-Cap
(`MAX_SUBSTITUTION_DEPTH`, Default 32) überschreitet, wörtlich vor: "Das
Kommando wird als nicht sicher parsebar behandelt und landet bei
`Confirm`". Die erste Umsetzung (Commit fd5b45a) folgte diesem Wortlaut
exakt.

Ein unabhängiger `spec-reviewer`-Review dieses Commits fand daraufhin
einen adversarialen Fall, der zeigt, dass `Confirm` hier keine fail-safe
Eskalation ist, sondern eine faktische Abschwächung: Der Tiefen-Cap
prüft, BEVOR weiter in verschachtelte Substitutionen abgestiegen wird —
das ist exakt richtig gegen Stack-Overflow (Fund B selbst), bedeutet aber
auch, dass eine Nutzer-/Organisations-`Deny`-Regel (Spec 0002/0037) auf
einem inneren Teilkommando ab Verschachtelungstiefe 33 nicht mehr
gefunden wird — die Rekursion, die sie finden würde, bricht ja genau
deshalb ab.

Reproduziert mit einer aktiven Regel `Deny "rm *"` und einem nur ~270
Zeichen langen Kommando (weit unter `DEFAULT_MAX_COMMAND_LENGTH` = 4096,
der Längen-Cap allein hätte hier also NICHT gegriffen):

```
echo $(echo $(echo $( … 33-fach … rm -rf / … )))
```

Vor dem Fix (`Confirm` bei Cap-Überschreitung): das Kommando landet bei
`Confirm`, obwohl eine echte Shell `rm -rf /` ausführen würde und die
`Deny`-Regel genau das verhindern soll. Erreichbar über einen
kompromittierten/prompt-injizierten KI-Provider oder MCP, der einfach
genug `$(echo ...)`-Wrapper um den eigentlichen Befehl legt.

Das verletzt zwei projektweite Garantien:

- CLAUDE.md, Abschnitt "Security-kritische Module": "Escalation only goes
  one direction: a rule-based `Deny`/risk-`Red` must never be downgraded
  by an AI opinion or a new code path."
- Spec 0002, Abschnitt 3: "ein `Deny` wird gar nicht erst zur Bestätigung
  angeboten".

## Entscheidung

Der Tiefen-Cap-Zweig in `FilterEngine::evaluate_parsed_explained`
(`crates/core/src/filter/engine.rs`) liefert `Deny` statt des in Spec
0043 §3 vorgeschlagenen `Confirm`, mit Code `FILTER_SUBSTITUTION_TOO_DEEP`
— dieselbe Fail-safe-Richtung wie der direkt darunterliegende `Empty`-Fall
("kein sinnvolles Kommando" → `Deny`, nicht `Confirm`). Begründung: ein
Kommando, das nicht mehr sicher bis zu seinem tatsächlichen Kern
analysiert werden kann, kann per Definition JEDES beliebige innere
Kommando verstecken — die einzige Entscheidung, die keine der beiden
Richtungen (zu streng für harmlose Fälle / zu lax für gefährliche Fälle)
systematisch bevorzugt, ist die, die eine bestehende `Deny`-Regel nicht
unterlaufen kann. Kein legitimes Kommando verschachtelt 33+ Ebenen tief
(Spec 0043 selbst nennt den Cap "großzügig genug für jeden legitimen
Fall") — das Risiko eines Fehlalarms bei echten Nutzerkommandos ist
praktisch null.

`code_priority` (für `merge_codes` bei kombinierten Entscheidungen) wurde
entsprechend verschoben: `FILTER_SUBSTITUTION_TOO_DEEP` steht jetzt auf
Stufe 1 (direkt nach `FILTER_HARD_BLACKLIST`), nicht mehr unterhalb von
`FILTER_COMMAND_SUBSTITUTION` — durch `Deny`s strikt höhere Severity in
`combine()` wird der Code eines dominierenden `Deny`-Tiefen-Cap-Befunds
ohnehin nie von einem schwächeren `Confirm`-Substitutions-Befund verdeckt,
die Prioritäts-Anpassung ist nur für den Fall zweier gleichzeitiger
`Deny`-Quellen relevant.

Regressionstest: `test_t43_deep_nesting_cannot_downgrade_a_deny_rule_to_confirm`
in `crates/core/src/filter/tests.rs` — belegt exakt das oben beschriebene
Szenario.

## Konsequenzen

- Ein zu tief verschachteltes Kommando ist ab sofort IMMER `Deny`, auch
  wenn keine Nutzerregel überhaupt betroffen wäre (reiner Fund-B-Schutz
  vor Stack-Overflow ohne beteiligte `Deny`-Regel) — geringfügig
  strenger, als Spec 0043 §3 wörtlich vorschlägt, aber konsistent mit dem
  `Empty`-Fall direkt daneben und mit keinem erwartbaren Fehlalarm für
  echte Nutzung.
- Spec 0043 selbst wird NICHT geändert (der committete Spec-Text bleibt
  die ursprüngliche Absicht dokumentieren) — dieses ADR hält die
  Abweichung und ihre Begründung fest, wie in CLAUDE.md, Abschnitt
  "Spec-first workflow" für genau diesen Fall vorgesehen ("eine
  bewusste Design-Entscheidung ... sag das explizit, statt den Scope
  stillschweigend zu verengen").

---

# Zusatzbefund: PTY-Scrollback (Spec 0043, Fund A, Abschnitt 2, dritter
Punkt — "prüfen und melden")

Spec 0043 verlangt für den interaktiven PTY-Modus explizit eine Prüfung,
ob dort dieselbe Ressourcen-Erschöpfungsklasse wie bei Fund A (unbegrenzt
wachsender Puffer VOR jeder Cap-Anwendung) vorliegt, und — falls ja — eine
Meldung (Fix nur, falls klein und derselben Klasse).

**Befund: nein, keine analoge Erschöpfungsklasse.**

- `RusshShell::read()` (`crates/ssh-transport/src/shell.rs`) puffert
  nicht: jede `ChannelMsg::Data`/`ExtendedData` wird als eigener Chunk
  sofort zurückgegeben (`return Ok(data.to_vec())`), es gibt keinen
  Zwischenspeicher, der über mehrere `read()`-Aufrufe hinweg wächst.
- `LocalShell::read()` (`crates/ssh-transport/src/local.rs`, lokaler
  Pseudo-Server) liest in einen festen `8192`-Byte-Stack-Puffer pro
  Aufruf, ebenfalls kein wachsender Zwischenspeicher.
- Die einzige tatsächliche Pufferung liegt im Frontend: `xterm.js`
  (`TerminalView.tsx`) wird ohne explizite `scrollback`-Option
  instanziiert, nutzt also `xterm.js`' eigenen Default von 1000 Zeilen —
  bereits von Haus aus begrenzt.

Kein Fix nötig. Dieser Abschnitt dient als das von Spec 0043 §2 geforderte
schriftliche "gemeldet".
