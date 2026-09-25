# ADR 0074 — Entscheidungen beim `ssh_config`-Import (Spec 0075, Schritte 0–3)

Status: angenommen · 2026-09-25 · Spec: `docs/specs/0075-ssh-config-import-export.md`
Betrifft: `crates/core/src/profiles/ssh_config/`,
`crates/app-shell/src/ssh_config_import.rs`,
`crates/app-shell/src/ssh_config_apply.rs`

Dieses ADR hält fest, was die Spec offen ließ oder was beim Umsetzen der
Schritte 0 bis 3 zu entscheiden war. Schritt 4 ff. (Schreiber/Export,
Frontend, Übersetzungen) sind nicht Teil dieses Laufs.

## 1. Eigener Parser statt `ssh2-config` (Spec §4.2, §9/M-1, §9/Q-1)

Entschieden **durch die Messung**, die §7.0 verlangt, nicht durch
Geschmack. E-1 hatte die Abhängigkeit ausdrücklich an diese Messung
geknüpft; sie fiel schlecht aus. Die Befunde stehen vollständig in §9/M-1
der Spec, die zwei tragenden hier:

- `ignored_fields`/`unsupported_fields` sind `HashMap<String, Vec<String>>`
  — **keine Zeilennummern**, und als `HashMap` keine Lesereihenfolge.
  §3.1.5 verlangt Datei **und** Zeilennummer; das ist strukturell nicht
  nachrüstbar.
- `Include` wird intern aufgelöst, gegen `$HOME/.ssh`, ohne Tiefenzähler
  und ohne Besuchtmenge. Eine sich selbst einbindende Datei beendete im
  Messprogramm den Prozess mit `fatal runtime error: stack overflow,
  aborting` — nicht per `catch_unwind` abfangbar, §6.4.5 (d) also nicht
  einmal als Test schreibbar.

Dazu der Befund, der „nur die `Host`-Blöcke übernehmen" ausschloss:
`Match` wird in den **vorhergehenden** `Host`-Block gemischt (`Host a` /
`User x` + `Match host b` / `User y` ⇒ `a.user == "y"`). Schon die
übernommenen Felder wären falsch gewesen.

**Folge:** kein Parser-Paket, und auch kein Ersatzpaket (§9/Q-1 Punkt 1).
Das Messprogramm lag unter `.agent/BL-0216/probe/` und ist nicht
committet.

## 2. Der Musterabgleich ist eigener Code, nicht `globset`

`globset` liegt im Baum, wäre also billig gewesen. Für `Host`-Muster
trotzdem nicht benutzt (§9/Q-1 Punkt 2): OpenSSH kennt dort `*`, `?` und
`!`; `globset` bringt `[…]` und `{…}` mit und kennt die Verneinung nicht.
`Host web[1` wäre ein Übersetzungsfehler statt eines wörtlichen Namens,
und `Host web[1-9]` träfe Server, die `ssh` nie trifft.

Der Abgleich ist ein Zwei-Zeiger-Verfahren ohne Rekursion. Grund: Ein
Muster kommt aus einer fremden Datei, und ein rückverfolgender Abgleich
ließe sich mit `*a*a*a*a*b` in exponentielle Laufzeit treiben — §5.6
schließt das aus. Ein Test hält die Laufzeit fest.

Für Platzhalter im **`Include`-Pfad** wird `globset` benutzt; dort ist
seine Grammatik die richtige, weil `glob(3)` dasselbe kann. Es wurde
dafür zur direkten Abhängigkeit von `app-shell` — kein neues Paket, nur
eine transitive Kante, die direkt wird (Muster wie `libc`, Spec 0076
K-3).

## 3. Ein Platzhalterblock ist ein Block, sobald **eine** Angabe einen trägt

§3.1.3 sagt, ein `Host`-Muster mit `*`, `?` oder `!` wird kein Profil.
Offen blieb `Host web1 *.de` — gemischt. Entschieden: Der **ganze** Block
gilt als Platzhalterblock, es entsteht also auch kein Profil `web1`.

Grund: Die Alternative (`web1` als Profil, `*.de` als Vorgabe) wäre eine
Sonderregel, die in der Spec nicht steht, und ein Block, der beides
mischt, ist erfahrungsgemäß eine Vorgabe und keine Serverliste. Die
Richtung ist die vorsichtige: Es entsteht ein Profil **weniger**, nicht
eines mit geratenen Feldern.

## 4. Was „erkannte Direktive" in §3.1.4a heißt

Zwei Ebenen, wie §9/Q-1 Punkt 3 sie verbindlich macht: Über die **Datei**
entscheidet eine Schlüsselwortliste, über „Name oder `unlesbare Zeile`"
nur noch die Form `[A-Za-z][A-Za-z0-9-]{0,31}`.

Die Liste ist **bewusst kurz**. Jedes Wort darin ist ein Wort, mit dem
eine Prosazeile beginnen und die Datei damit als `ssh_config` durchgehen
lassen könnte. Eine Lücke kostet dagegen nur, dass eine dünn besetzte
echte `ssh_config` übersprungen und mit ihrem Pfad gemeldet wird — die
Richtung, in der nichts nach außen gelangt. `Compression` steht darin,
weil §9/Q-1 Punkt 7 es für den Prüfling von §6.4.9a verlangt.

Der Gegenbeweis hat gezeigt, dass das nicht theoretisch ist: Stellt man
die Einordnung auf die **Form** um, wird eine Datei mit einem privaten
Schlüssel als `ssh_config` eingestuft und ihre Base64-Zeile als
Direktivenname gemeldet — Schlüsselmaterial auf dem Schirm, genau der
Kanal, den §5.4/E-7 zugemacht hat.

## 5. Drei Annahmen, die eine Entscheidung brauchen könnten

**ANNAHME A-1 — `Include` in einem `Match`-Block wird gemeldet, nicht
gefolgt.** §3.1.4 sagt „`Include` wird gefolgt", §4.2 sagt „`Match` wird
nicht ausgewertet". Zusammen ist offen, was mit einem `Include`
**innerhalb** eines `Match`-Blocks geschieht. Gewählt: melden statt
öffnen. Weil wir die `Match`-Bedingung nicht auswerten, wüssten wir
nicht, ob `ssh` sie erfüllt sähe; die Datei zu öffnen hieße, eine Datei
zu lesen, die der Nutzer selbst vielleicht nie liest. §5.1 („von sich aus
öffnet der Import ausschließlich `ssh_config`-Dateien") zeigt in die
Richtung, in der weniger geöffnet wird.
*Aufgelöst durch:* eine Entscheidung des PO, falls `Match`-Includes
gefolgt werden sollen. Kostet dann eine Zeile.

**ANNAHME A-2 — Platzhalter nur in der letzten Pfadkomponente.**
`Include conf.d/*.conf` wird aufgelöst (der Fall, den §3.1.4 nennt und
§6.2.3 prüft); ein Platzhalter weiter vorne (`Include /*/*/x.conf`) wird
als nicht übernommen gemeldet. Grund: Ein solches Muster aus fremder Hand
würde einen Verzeichnisbaum durchlaufen, den niemand begrenzt hat — das
liefe §5.6 zuwider.
*Aufgelöst durch:* eine Entscheidung, ob die Fidelität zu `glob(3)` diesen
Durchlauf wert ist. Dann braucht es eine eigene Grenze dafür.

**ANNAHME A-3 — ein ungültiger `Port` wird gemeldet, nicht stillschweigend
zu 22.** §3.1.2 regelt nur den **fehlenden** `Port`. Ein `Port abc` oder
`Port 99999` fällt auf die Vorgabe 22 zurück **und** erscheint unter
„nicht übernommen". Stillschweigend auf 22 zu fallen wäre die Art von
Verschlucken, die BL-0216 ausschließt.

## 6. Der Importplan bleibt im Backend

§5.1 verlangt, dass auf Weg (b) **nur** die Dateien geöffnet werden, die
die Vorschau vorher genannt hat — „auch dann nicht, wenn sich die
Konfigurationsdatei zwischenzeitlich geändert hat". Käme der Plan beim
Bestätigen aus dem Frontend zurück, wäre das nicht haltbar: Beliebiger
Code im Webview könnte einen anderen Pfad einsetzen.

Deshalb liegt der Plan zwischen Vorschau und Bestätigen im `AppState`,
und `apply_ssh_config_import` nimmt **nur Indizes**. Das ist dieselbe
Konsequenz, die `commands::read_credential_file` nach dem Fund SEC-06
(Spec 0013) schon gezogen hat; der Dateidialog läuft entsprechend
ebenfalls im Backend.

Nebenwirkung, die bleibt: Nach einem Neustart der App ist eine offene
Vorschau verfallen. Das ist richtig so — ein Plan, der auf Dateien zeigt,
die seither jemand geändert haben kann, soll nicht ausführbar sein.

## 7. Weg (a) und (c) öffnen nichts, und das ist geprüft

`KeyFileSource` ist ein Trait, nicht ein direkter `std::fs`-Aufruf. Nur
damit ist die Aussage „die Vorschau und Weg (a) öffnen **keine** Datei"
überhaupt prüfbar: Der Test zählt die geöffneten Pfade mit. Der
Gegenbeweis (Weg (a) liest „hilfsbereit" doch) lässt §6.4.1 scheitern.

## 8. Ein Jump-Ziel, das der Nutzer abwählt, ergibt keinen Jump-Host

Offen gelassen von der Spec: Was passiert, wenn `x` über `bastion` geht
und der Nutzer `bastion` in der Vorschau abwählt? Gewählt: `x` entsteht
**ohne** Jump-Host. Die Alternativen wären, `x` ebenfalls zu
unterdrücken (nimmt dem Nutzer eine Entscheidung ab, die er getroffen
hat) oder auf ein nicht existierendes Profil zu zeigen (geht nicht, der
Fremdschlüssel verbietet es). Ohne Jump-Host ist der Zustand, den der
Nutzer danach von Hand richten kann — und er ist sichtbar, nicht falsch.

## 9. Was dieser Lauf nicht entschieden hat

Der `ProxyJump`-Hop wird im Bestand **erst über den Namen, dann über die
Adresse** aufgelöst. Die Spec sagt nur „vorkommt". In einer `ssh_config`
steht in `ProxyJump` üblicherweise ein Alias, deshalb hat der Name
Vorrang. Falls das einmal zu einem falschen Treffer führt, ist die
Reihenfolge die Stelle, an der man ansetzt.
