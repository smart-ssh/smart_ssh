# ADR 0085 — Entscheidungen bei der Umsetzung von Spec 0093

Status: akzeptiert (A1–A2, A5–A7) · Spec:
`docs/specs/0093-windows-ci-last-two.md` · Backlog: BL-0281

## 1. A2 — Manifest-Einbettung: zwei getrennte Wege statt einem gemeinsamen

`crates/app-shell/build.rs` unterscheidet `target_env = "msvc"` und `"gnu"`
und geht für beide einen anderen Weg, statt einen gemeinsamen
Ressourcen-Compiler-Weg für beide zu suchen:

- **MSVC:** `cargo:rustc-link-arg=/MANIFEST:EMBED` plus
  `/MANIFESTDEPENDENCY:…` — der Linker baut und bettet das Manifest selbst,
  ohne Ressourcen-Datei und ohne externes Werkzeug.
- **GNU:** `windres` kompiliert ein `.rc`, das auf eine erzeugte
  `.manifest`-Datei verweist, zu einem COFF-Objekt, das dann per
  `cargo:rustc-link-arg` gelinkt wird — der einzige gemessene Weg (Spec
  0093, Ist-Stand 4), da `ld` kein `/MANIFEST` kennt.

Beide Zweige verwenden dieselbe Manifest-Abhängigkeit
(`Microsoft.Windows.Common-Controls`, Version 6.0.0.0). Der MSVC-Zweig ist
**nicht** per CI-Lauf nachgemessen (das ist Teil 0 der Spec, dem Architekten
zugewiesen) — nur der Import (`TaskDialogIndirect` aus derselben
Abhängigkeit) ist derselbe wie beim gemessenen GNU-Zweig, das trägt die
Annahme, dass beide Wege zum selben Ergebnis führen.

Keine neue Abhängigkeit: beide Zweige verwenden nur `std::process::Command`
und `std::fs`, kein `embed-manifest` (scheidet laut Spec aus), kein
`winres`/`embed-resource`.

## 2. A6 — Neues Zeitlimit, hergeleitet aus Messung

Das Limit in `test_secret_check_stays_fast_on_adversarial_long_input`
(`crates/core/src/risk/tests.rs`) stieg von 3s auf 10s. Herleitung und
Messwerte stehen direkt im Kommentar über dem Test, hier nur die
Zusammenfassung: lokal (macOS, Debug-Build) lief die langsamste der vier
Eingaben in ~0,45s, `windows-latest` riss bei 3,01s (Faktor ~6,7 zum
lokalen Wert) — 10s lässt reichlich Marge zu beidem.

**Gegenprobe (Spec 0093, T7) mit ehrlichem, nicht wie erwartet
ausgefallenem Ergebnis:** Die Rekursionstiefen- und Budget-Schranke in
`extended_secret_read_reason_in` (`depth > 3`,
`MAX_NESTED_CHECKS`) testweise stark gelockert (`depth > 30`, Budget
`1_000_000`) und dieselben vier Adversarial-Muster bei 25/50/75/100 % ihrer
Wiederholungszahl erneut gemessen: Keines zeigte exponentielles Wachstum,
das schlechteste blieb linear (908 → 3608 Zeichen: 133ms → 454ms). Grund:
Das `seen`-Set in `NestedBudget` dedupliziert exakt wiederholte
Teilstrings, und der Längen-Cutoff `DEFAULT_MAX_COMMAND_LENGTH` gilt bei
jedem rekursiven Aufruf erneut — beide zusammen verhindern eine
kombinatorische Explosion bei diesen (identisch wiederholten) Mustern
bereits unabhängig von Tiefe/Budget. Die Tiefen-/Budget-Schranke selbst
bleibt unverändert (nicht Teil der Spec); der Zeittest bleibt als
unabhängiges zweites Netz bestehen, falls ein künftiger Rückfall Dedup
oder Längen-Cutoff selbst aufhebt.

## 3. A7 — `flush()` statt `fsync()`

Der Testserver bestätigt `write`/`close` jetzt erst nach `AsyncWriteExt::
flush()` (`crates/ssh-transport/tests/fixtures/test_server.rs`), nicht nach
einem Betriebssystem-`fsync`. Grund: Der beschriebene Fehler war rein
`tokio`-intern (gepufferte Schreib-Tasks, deren Abschluss `write_all`
selbst nicht abwartet) — sobald `flush()` zurückkehrt, ist der Inhalt über
den regulären Datei-Deskriptor sichtbar, unabhängig davon, ob er schon auf
physischer Platte liegt. Für einen Test-Prozess auf demselben Rechner
reicht das; `fsync` wäre eine stärkere (und langsamere) Garantie, die die
Spec nicht verlangt.

**Gegenprobe (Spec 0093, T8) — ehrliches Ergebnis:** Der Fix wurde
temporär entfernt und sowohl der einzelne Roundtrip-Test als auch das
ganze `ssh-transport --test integration`-Binary je 20× wiederholt — beide
Läufe blieben grün. Der ursprünglich gemeldete Fehler ist als sehr selten
dokumentiert ("einmal rot in einem von zwei vollen Gate-Läufen"); 20
Wiederholungen reichten nicht, ihn erneut auszulösen. Der Fix behebt die im
Code klar identifizierbare Ursache (fehlendes `flush()` vor der
SFTP-Bestätigung), ist aber nicht durch einen tatsächlich rot gewordenen
Gegenbeweis belegt — das wird hier bewusst offengelegt statt verschwiegen.

## 4. A5 — begrenzte Wiederholung statt gebundenem, nicht lauschendem Socket

**Gemessen, nicht nur gelesen:** Ein per `tokio::net::TcpSocket` gebundener,
aber nie in `listen()` versetzter Socket (also `bind()` ohne `listen()`,
wie in Spec 0093 A5 ursprünglich als Beispiel genannt: "etwa ein gebundener
Socket ohne `listen()`") liefert auf macOS (Darwin) für einen
`connect()`-Versuch dagegen **keinen** `ConnectionRefused` — der
Verbindungsversuch läuft in einen Timeout (20/20 Testläufen). Vermutung
(nicht weiter verifiziert): Der TCP-Stack von macOS behandelt einen
gebundenen-aber-nicht-lauschenden Socket anders als einen Port ganz ohne
Socket — Letzterer liefert sofort ein RST, Ersterer lässt das SYN
offenbar unbeantwortet.

Damit hätte der ursprünglich vorgeschlagene Weg das geforderte Verhalten
(„Er prüft weiterhin `ConnectionRefused`, auf allen drei Plattformen") auf
mindestens einer der drei Zielplattformen nachweislich nicht erreicht. Ob
Linux/Windows sich anders verhalten, ist nicht geprüft (kein Zugriff auf
diese Plattformen in dieser Sitzung) — es blieb bei der einen Messung, die
den vorgeschlagenen Weg bereits ausschließt.

**Entschieden (Q-BL-0281-01, Spec 0093 §9):** A5 wird stattdessen über eine
begrenzte Wiederholung im Test selbst gelöst
(`test_connect_to_closed_local_port_yields_connection_refused`,
`crates/ssh-transport/tests/integration.rs`). Der Test bindet weiterhin
einen Port und gibt ihn sofort wieder frei; nimmt der so freigewordene
Port unerwartet eine Verbindung an (`Ok(_)`, weil ein fremder Testserver
ihn zwischenzeitlich belegt hat), verbindet er sich mit einem frischen
Port erneut, höchstens fünfmal. Jeder andere Fehler als
`ConnectionRefused` lässt den Test sofort scheitern, ohne Wiederholung.
Scheitern alle fünf Versuche am fremden Dienst, wird der Test rot, mit
einer Meldung, die das benennt.

**Gegenprobe (Spec 0093, T6), zweistufig, mit ehrlichem Ergebnis:**
1. Ein Listener, der auf dem gewählten Port gebunden bleibt, statt ihn
   freizugeben (`bind()` ohne `listen()`, wie oben gemessen), lässt den
   Test rot werden — allerdings über den 15-Sekunden-Timeout der
   Verbindung, nicht über die Wiederholungs-Erschöpfung, da das
   SSH-Handshake auf einen nicht lauschenden Port nie antwortet.
2. Ein echter, laufender Test-SSH-Server (`RunningTestServer::start()`)
   anstelle des freigegebenen Ports lässt jeden der fünf Versuche mit
   `Ok(_)` zurückkehren (er nimmt die Verbindung tatsächlich an) und der
   Test scheitert nach dem fünften Versuch in 0,05 s mit der vorgesehenen
   Meldung „alle 5 Versuche gerieten an einen fremden Dienst" — das ist
   der Fall, den die Klarstellung eigentlich beschreibt (ein fremder
   Testserver belegt den Port), und die Wiederholungslogik verhält sich
   dabei wie vorgesehen.

Beide Varianten wurden nur für die Gegenprobe eingebaut und wieder
entfernt, nicht committet. Der Portfall selbst ist mit 20 aufeinander-
folgenden grünen Läufen von `cargo test -p ssh-transport --test
integration` belastet nachgewiesen (T6).
