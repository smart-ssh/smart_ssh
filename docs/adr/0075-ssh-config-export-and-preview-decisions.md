# ADR 0075 — Entscheidungen bei Export und Vorschau des `ssh_config`-Austauschs (Spec 0075, Schritte 4–6)

Status: angenommen · 2026-09-25 · Spec: `docs/specs/0075-ssh-config-import-export.md`
Betrifft: `crates/core/src/profiles/ssh_config/export.rs`,
`crates/core/src/profiles/ssh_config/quoting.rs`,
`crates/app-shell/src/ssh_config_export.rs`,
`crates/app-shell/src/ssh_config_apply.rs`,
`apps/smart-ssh-community/frontend/src/components/SshConfigImportDialog.tsx`,
`apps/smart-ssh-community/frontend/src/components/SshConfigExportDialog.tsx`

Setzt ADR 0074 fort (dort: Schritte 0–3, Parser/Abbildung/Dateizugriff/
Kommandos). Dieses ADR hält fest, was Schritte 4–6 offen ließen oder wo
zu entscheiden war — inklusive der Funde aus zwei Spec-Reviewer-Runden
(`ERHÖHT`, `.agent/BL-0216/review-03.md` und `review-04.md`) und wie sie
triagiert wurden.

## 1. Alias-Zählweise bei leerem Namen (§4.3)

§4.3 sagt „ist das Ergebnis leer, wird `server-<n>` benutzt; bei Kollision
wird `-2`, `-3`, … angehängt", legt aber nicht fest, wie `<n>` gezählt
wird. Entschieden: ein Zähler über alle in **diesem** Export entstandenen
leeren Fällen (`server-1`, `server-2`, …), danach greift bei einer
Kollision mit einem echten Namen oder einem anderen `server-<n>` dieselbe
`-2`/`-3`-Regel wie sonst. Deterministisch, für den Rundlauf ausreichend,
kein zweiter Mechanismus nötig.

## 2. `ProxyJump`-Zeile fehlt, wenn das Ziel nicht exportiert wird

Zeigt `jump_host` auf ein Ziel, das nicht mit exportiert wird, entsteht
keine `ProxyJump`-Zeile statt einer, die auf einen nicht existierenden
Alias zeigt. Dieser Fall ist mit dem bestehenden Fremdschlüssel und
`SERVER_JUMP_HOST_LOCAL` (§5.7) nach heutigem Stand nicht erreichbar (der
lokale Pseudo-Server kann nicht als Jump-Host gesetzt sein) — die
Auslassung ist defensiv, kein beobachtetes Verhalten.

## 3. Der Export bekommt keinen Vorschau-Dialog

§3.1.7 verlangt eine Vorschau ausdrücklich für den **Import**; für den
Export verlangt die Spec nur eine Meldung danach (§3.2.5, §3.2.7). Der
Export-Dialog (`SshConfigExportDialog.tsx`) öffnet deshalb direkt den
Speichern-Dialog und zeigt erst das Ergebnis (Pfad, umbenannte Aliase,
`Include`-Zeile) — kein zweistufiger Ablauf wie beim Import.

## 4. `HostName` entfällt bei leerem `host`

§3.2.1 nennt `HostName <host>` ohne Bedingung (anders als `Port`/`User`/
`ProxyJump`, wo die Spec die Bedingung ausdrücklich hinschreibt) — die
Spec geht also von einem nicht-leeren `host` aus. §1.3 hält aber fest,
dass das Anlegen von Hand keinen Pflichtfeld-Check kennt; ein leerer
`host` ist real erreichbar. `HostName ""` ist gegenüber echtem `ssh`
fragwürdig (abhängig von der Version, ob `ssh -G` das akzeptiert).
Entschieden: Die Zeile bleibt bei leerem `host` ganz weg — `ssh` verhält
sich dann wie ohne `HostName` (nimmt den Alias). Das ist die
konservativere Verallgemeinerung eines Falls, den die Spec nicht bedacht
hat, keine Abweichung von einem Fall, den sie regelt. Getestet:
`leerer_host_erzeugt_keine_hostname_zeile`.

## 5. Leser/Schreiber-Asymmetrie bei `\` bleibt bestehen — und ist gewachsen

§9/Q-1 Punkt 5 verlangt, dass Leser und Schreiber „eine Zitier- und
Trennerroutine teilen". `quoting.rs` wird ausschließlich vom Schreiber
(`export::quote_value`) benutzt; `parser.rs::tokenize` bleibt unverändert
und entschachtelt kein `\"`. Das ist so entschieden: Der Parser ist Teil
der bereits geprüften und gemergten Schritte 1–3 (ADR 0074), der Auftrag
für Schritte 4–6 sagt ausdrücklich „Kern nicht umbauen", und eine Änderung
an sicherheitskritischer, bereits geprüfter Leselogik für einen
Randfall (ein Wert mit eingebettetem `"` **und** `\`) steht außer
Verhältnis zum Nutzen.

Runde 1 des Reviews fand, dass `quote_value` einen Wert, der auf `\`
endet, falsch maskierte (`"a b\"` statt `"a b\\"` — das Escapezeichen
fraß das schließende Anführungszeichen). Behoben: `\` wird jetzt vor `"`
maskiert, verifiziert gegen echtes `ssh -G` (nicht nur den eigenen
Leser). Runde 2 wies darauf hin, dass diese Korrektur die Asymmetrie zum
eigenen Leser **vergrößert** hat: Vorher waren beide Seiten bei `\`
„falsch einig" (roh, kein Escaping), jetzt maskiert der Schreiber `\` →
`\\`, ohne dass der Leser das zurückrechnet — ein `IdentityFile`-Pfad mit
`\` (z. B. ein Windows-Pfad) rundläuft deshalb nicht mehr sauber durch
unseren **eigenen** Parser, auch wenn er (das ist der Maßstab, den
§9/Q-1 setzt) gegen echtes `ssh` korrekt ist.

Bewertung: **bewusst nicht vollständig behoben.** Der äußere Zeuge
(`ssh -G`) ist der maßgebliche Prüfling und besteht (`t_review1_quote_value_maskiert_eingebettetes_anfuehrungszeichen`,
`t_review1_quote_value_maskiert_trailing_backslash`); der Rundlauf durch
den eigenen Leser ist für Werte mit `"` oder `\` eine bekannte, benannte
Lücke. Backlog-Punkt: `parser.rs::tokenize` um Escape-Auflösung
(`\"`, `\\`) erweitern und §6.3.3 um einen entsprechenden Wert ergänzen —
das ist eine Änderung an Schritt 1, nicht an diesem.

## 6. `~/.ssh/config`-Ablehnung: additive Prüfungen, nie ersetzt

Runde 1 fand zwei Umgehungen der ursprünglichen (lexikalisch + aufgelöstes
Elternverzeichnis) Prüfung: unterschiedliche Groß-/Kleinschreibung
(`~/.ssh/Config` auf case-insensitiven Dateisystemen) und ein Symlink auf
die Zieldatei selbst (vorher wurde nur das Elternverzeichnis aufgelöst).
Runde 2 fand eine dritte: ein Hardlink auf die Zieldatei, den
`canonicalize` grundsätzlich nicht auflöst (verschiedene Inodes haben
unterschiedliche kanonische Pfade auch bei gleichem Inhalt).

Alle drei Nachbesserungen sind **additiv** — jede neue Prüfung gibt nur
zusätzlich `true` zurück, keine bestehende wurde verändert oder entfernt
(„Eskalation nur in eine Richtung", ADR 0024-Muster). `resolves_to_ssh_config_under`
prüft jetzt fünf Fälle: lexikalisch exakt, lexikalisch
case-insensitiv (mit gleichem Elternverzeichnis), aufgelöster Pfad selbst,
aufgelöstes Elternverzeichnis case-insensitiv, und (nur Unix) dieselbe
`(device, inode)`-Kombination. Getestet mit einem injizierten
`tempdir()`-Home statt dem echten `~` dieser Maschine, damit die Fälle
unabhängig davon prüfbar sind, ob `~/.ssh` auf der Bau-Maschine existiert.

Offen, nicht gemessen (Runde 2): Ob `canonicalize` unter macOS/Windows
bei einer case-insensitiven Abweichung im **Elternverzeichnis** selbst
(`~/.SSH/config`) die tatsächliche Schreibweise auf der Platte
zurückgibt. Wenn nicht, greift dort nur die Ablehnung über Fall 1/2
(gleiche Schreibweise) — harmlos, weil ohne existierendes `~/.ssh` auch
keine Datei zu schützen wäre.

## 7. Kommentar- und Wert-Ausbruch über Steuerzeichen — Kern der ERHÖHT-Runde 1

Der schwerwiegendste Fund der ersten Review-Runde: Gruppenname (= Name der
Importdatei, roh übernommen), Schlagworte und `sftp_server_path` gingen
ungeprüft in `# smart-ssh:`-Kommentarzeilen; ein `\n` darin beendete die
Kommentarzeile vorzeitig, der Rest der Zeichenkette stand als **eigene,
wirksame** Zeile in der Datei — vor dem ersten `Host`-Block platziert,
galt sie („der erste gewinnt") für jeden Host der Datei. Derselbe Fehler
betraf jeden über `quote_value` geschriebenen **Wert** (z. B. `username`),
nicht nur Kommentare — dort brach der Zeilenumbruch den Block in zwei
Teile.

Entschieden: eine gemeinsame Funktion (`quoting::strip_control_chars`,
ursprünglich `strip_line_breaks`, in Runde 2 erweitert) ersetzt jedes
C0-Steuerzeichen — nicht nur `\n`/`\r` — durch ein Leerzeichen, **bevor**
ein Wert gequotet oder in einen Kommentar gesetzt wird. Grund für die
Erweiterung über `\n`/`\r` hinaus (Runde 2): Ein `\0` bricht zwar keine
Zeile auf, aber echtes `ssh` schneidet einen Wert dort ab, und die eigene
exportierte Datei würde beim Wiedereinlesen unter §3.1.4a fallen („enthält
ein NUL-Byte") — eine Datei, die sich selbst nicht mehr importieren
lässt, ohne dass irgendwo eine Meldung entsteht.

Eine allgemeine Kontrollzeichen-**Eingangs**politik für `ServerInput`/
Gruppenanlage (die verhindern würde, dass ein Server-Feld überhaupt ein
Steuerzeichen enthält) wäre die umfassendere Lösung, ist aber ein
separater, größerer Punkt (berührt Spec 0008/0032) und **nicht**
Gegenstand dieses Schritts — hier greift ausschließlich die Verteidigung
im Schreiber.

## 8. §3.1.9 (b) „Passphrase nachzutragen" ist ein Nach-Apply-Signal, kein Vorschau-Signal

§3.1.9 (b) sagt wörtlich: „Die Vorschau sagt bei solchen Einträgen, dass
die Passphrase nachzutragen ist." §5.1 (und Nicht-Ziel 5, §6.4.1b) sagen
ebenso wörtlich, dass eine Schlüsseldatei **nie in der Vorschau**, nur
beim Bestätigen geöffnet wird. Ob ein konkreter Schlüssel verschlüsselt
ist, lässt sich ohne Öffnen nicht feststellen — die beiden Sätze der Spec
sind an dieser Stelle nicht gleichzeitig einlösbar (Fund aus
Review-Runde 2).

Entschieden, weil §5.1 die härtere, sicherheitstragende Aussage ist und
in der Spec an keiner Stelle relativiert wird: Das Signal steht im
`ApplyOutcome` (`identity_encrypted: Vec<{entry, path}>`), befüllt aus der
tatsächlichen Klassifikation beim Lesen auf Weg (b) — nach dem
Bestätigen, nicht davor. Der Ergebnis-Toast benennt Eintrag und Datei.
Zusätzlich (Runde 2, um die Vorschau nicht ganz stumm zu lassen): Der
Kasten, der die Dateien nennt, die auf Weg (b) geöffnet würden, trägt
jetzt einen **allgemeinen**, nicht dateispezifischen Hinweis, dass ein
verschlüsselter Schlüssel trotzdem übernommen wird und die Passphrase
danach nachzutragen ist.

**Das ist eine Auslegung, kein Ausweichen** — es gibt unter der
bestehenden §5.1-Invariante keine Alternative, die einen dateispezifischen
Vorschau-Hinweis liefert. Trotzdem: Der Widerspruch zwischen den beiden
Sätzen der Spec ist real und gehört als redaktionelle Klarstellung in
§9, sobald jemand mit Spec-Schreibrecht dort ansetzt — das ist keine
Entscheidung, die ein Coder-Lauf an der Spec selbst vornimmt. Vorgelegt
im Abschlussbericht dieses Laufs.

## 9. Q-BL-0216-02 entschieden: buchstäbliches Schlagwort mit Allow-Treffer standardmäßig abgewählt

ADR 0074, Punkt 5 (§5.2a) und Punkt 10.1 hatten die Richtung offen
gelassen, in die die Vorgabe für ein buchstäbliches Schlagwort
(`PlannedTag::is_literal`) zeigen soll, das eine bestehende Tag-Regel
trifft — vorgelegt als `Q-BL-0216-02`. Stefan hat entschieden (2026-09-25,
eingearbeitet in Spec §9):

- Trifft das Schlagwort eine bestehende Tag-**`Allow`**-Regel, ist es in
  der Vorschau standardmäßig **abgewählt**. Ohne diese Vorgabe würde ein
  Import stillschweigend ein Profil von `Confirm` auf `Allow` heben, nur
  weil ein gemischter Block wie `Host prod *` dem Server `prod` das
  buchstäbliche Schlagwort `prod` gibt (§5.2a).
- Trifft es nur eine `Deny`- oder `Confirm`-Regel, oder keine, bleibt es
  wie jedes andere Schlagwort **angewählt** — eine bestehende
  Tag-`Deny`-Regel bleibt damit ohne Zutun wirksam, genau das, was der
  Lockerungs-Gegencheck aus ADR 0074 Punkt 10.1 verlangt hatte.
- Der Nutzer kann in beide Richtungen umwählen.

Umgesetzt an der **einzigen** dafür vorgesehenen Stelle,
`defaultTagSelected` in `SshConfigImportDialog.tsx` — core liefert
`is_literal` und `matched_rules` (samt `action`) bereits vollständig, die
Vorgabe war zuvor bewusst neutral (`return true`) belassen, bis diese
Antwort vorlag. Getestet: ein buchstäbliches Schlagwort mit Treffer auf
eine `Allow`-Regel startet abgewählt, eines mit Treffer nur auf eine
`Deny`-Regel bleibt angewählt (`SshConfigImportDialog.test.tsx`,
„Q-BL-0216-02: a literal tag hitting an Allow rule starts deselected, one
hitting only Deny stays selected"); die beiden Bestandstests, die die
vorherige neutrale Vorgabe voraussetzten, sind an die neue Vorgabe
angepasst.

## 10. Was aus den Review-Runden stehen bleibt

**Runde 1 (11 Funde) — behoben:** Kommentar-/Wert-Ausbruch über
Steuerzeichen (Punkt 7 oben); `~/.ssh/config`-Umgehung per
Groß-/Kleinschreibung und Symlink auf die Datei (Punkt 6, Fälle 2/3);
`include_hint` jetzt gequotet; `quote_value` maskiert `\` vor `"` (Punkt
5); §5.2a nennt jetzt die Aktion (Allow/Confirm/Deny) der getroffenen
Regel, nicht nur eine Zahl; §3.1.9 (b) „Passphrase nachzutragen" als
Nach-Apply-Signal (Punkt 8); Fallback-Meldung nennt jetzt Eintrag und
Grund, nicht nur eine Zahl; der echte §6.3.1-Test (Vorschau==Ergebnis)
existiert jetzt in `ssh_config_apply/tests.rs`; §6.4.6a hat jetzt den von
der Spec verlangten `ssh -G`-Zeugen; tote „ADR zu diesem Schritt"-Verweise
zeigen jetzt hierher; interne Prozessvokabeln aus einem Code-Kommentar
entfernt.

**Runde 2 (Delta) — behoben:** Steuerzeichen-Politik von `\n`/`\r` auf
alle C0-Zeichen erweitert (Punkt 7); Hardlink-Erkennung ergänzt, falsche
„oder Hardlink"-Behauptung im Doc-Kommentar korrigiert (Punkt 6); Test
für die dritte Kommentar-Senke (`sftp_server_path`) ergänzt; Test für
§5.2a-Aktion (Rust **und** Frontend) ergänzt; §6.3.1-Test um
`identityFile`, `conflict` und Gesamtzahl erweitert; genereller
Passphrase-Hinweis in der Vorschau ergänzt (Punkt 8).

**Bewusst nicht (vollständig) behoben — mit Grund:**

1. **Leser/Schreiber-Asymmetrie bei `\`** (Punkt 5) — `parser.rs` bleibt
   unverändert, Backlog-Punkt benannt.
2. **§3.2.3 „Zusammenfassung im Kopf der Datei" bleibt generisch** (nennt
   Kategorien, nicht die konkret betroffenen Server) — beide Runden
   stuften das als Nacharbeit ein, kein Verstoß.
3. **Hardlink-/Symlink-Schutz für Exportziele außerhalb `~/.ssh/config`**
   (z. B. `export.conf` → `~/.ssh/authorized_keys`) — jenseits von §3.2.5,
   das nur die eine Datei benennt.
4. **Kontrollzeichen-Politik an der Eingangsgrenze** (`ServerInput`,
   Gruppenanlage) — separater, größerer Punkt (Spec 0008/0032).
5. **Fall „`~/.SSH/config`" (case-insensitives Elternverzeichnis) nicht
   empirisch gemessen** (Punkt 6, letzter Absatz) — als Unsicherheit
   benannt, harmlos ohne existierendes `~/.ssh`.
6. **Erfolgs-Toast bleibt `kind: "success"`**, auch wenn Schlüssel
   zurückfielen oder Passphrasen nachzutragen sind — der Toast-Bus kennt
   nur `"success"`/`"error"`, eine dritte Stufe („warning") wäre eine
   eigene, über diesen Schritt hinausgehende Änderung.

**Vorbestehend, nicht Gegenstand dieses Laufs, aber vor Stefan zu
bringen:** ANNAHME A-1/A-2/A-3 aus ADR 0074 (unbestätigt seit Schritten
0–3) und die offene Entscheidung Q-BL-0216-02 (§5.2a, Vorgabe für ein
buchstäbliches Schlagwort, das eine Allow-Regel trifft) — beide blieben
zum Zeitpunkt dieses Runs (Schritte 4–6) unverändert offen, s.
Abschlussbericht. *Beide sind seither aufgelöst:* A-1/A-2 haben Tests
(nachgezogen in diesem Nachlauf, s. ADR 0074 Punkt 5/8), Q-BL-0216-02 ist
entschieden (§9 oben).
