# ADR 0064: Entscheidungen bei der Umsetzung von Spec 0073 (geteilter Credential-Trim)

Status: Angenommen
Bezug: docs/specs/0073-shared-credential-trim.md, Commits `0b57d09` (Helfer),
`f754ab8` (DTO), `3beeba3` (Server-Secrets), `f3195bc` (adversariale Fälle),
`2a93c5e` (Review-Nacharbeit)

Spec 0073 verlangt kein eigenes ADR für eine offene Design-Entscheidung
(§8: „Offene Punkte: Keine"). Beim Umsetzen und beim Abschluss-Review sind
trotzdem Punkte aufgelaufen, die hierher gehören: eine Abweichung von einer
MUSS-Anforderung, drei bewusst nicht behobene Review-Funde und zwei
fachliche Festlegungen, die über den Auftrag hinausgingen.

## 1. `normalize_sftp_server_path` bleibt vorerst bei `str::trim` (Abweichung von A3)

**Frage:** A3 verlangt „Alle in §1 genannten Stellen benutzen den Helfer",
und §1 listet `dto.rs:514` — das ist `normalize_sftp_server_path`. Derselbe
Satz A3 schränkt aber auf Zugangsdaten-Werte ein, und A1/A5 beschreiben den
Helfer ausdrücklich als Helfer für Zugangsdaten. `sftp_server_path` ist
keiner: Es ist ein Pfad-Override für den erhöhten Dateibrowser (Spec 0067,
A2), der in ein `sudo`-Kommando und in die angezeigte sudoers-Zeile wandert.

**Entscheidung:** nicht umgestellt, markiert als
`ANNAHME A-1 (Q-BL-0149-01)` im Code. Die Entscheidung liegt beim
Produktverantwortlichen und ist zum Zeitpunkt dieses ADR offen.

**Warum nicht selbst entschieden:** Der Unterschied ist nicht kosmetisch.
`"/usr/lib/sftp-server\u{200B}"` wird heute **abgelehnt** — das ZWSP
übersteht `str::trim` und fällt dann durch `is_plausible_sftp_server_path`.
Mit dem Helfer würde derselbe Pfad bereinigt und **angenommen**. Die Prüfung
selbst bliebe unverändert und liefe in beiden Fassungen nach dem Trimmen,
der angenommene Endwert ist also in jedem Fall voll validiertes ASCII; es
ändert sich aber, ob eine solche Eingabe als Fehler oder als bereinigter
Pfad endet — an einem Pfad mit erhöhten Rechten. Das ist eine
Produktentscheidung, keine Ableitung aus der Spec.

**Nebenwirkung, falls umgestellt wird:** Eine Eingabe, die *nur* aus
unsichtbaren Zeichen besteht, ergäbe dann `Ok(None)` = „automatisch" statt
eines Fehlers — dieselbe Semantik, die `"   "` heute schon hat. Stimmig,
aber vorher zu benennen statt hinterher zu entdecken.

## 2. Die Zeichenliste aus A1 ist unvollständig — nicht erweitert (§8)

**Befund (Abschluss-Review):** Es gibt weitere unsichtbare Zeichen ohne
`White_Space`-Eigenschaft, die nicht in A1 stehen und deshalb stehen
bleiben: U+200E/U+200F (Bidi-Marks), U+202A–U+202E, U+2066–U+2069,
U+00AD (Soft Hyphen), ggf. U+180E. Für diese Zeichen besteht das
Eingangsszenario der Spec — ein Key, der richtig aussieht, aber mit 401
scheitert — unverändert fort.

**Entscheidung:** **nicht behoben.** Spec §8 legt ausdrücklich fest, dass
die Liste eine fachliche Festlegung ist, „die der Coder nicht erweitern
soll, ohne es zu melden". Hiermit gemeldet; die Erweiterung ist eine
Produktentscheidung.

Der Test `test_x3_invisible_chars_outside_the_list_are_kept` hält die
Endlichkeit der Liste bewusst fest, damit niemand sie später versehentlich
zu „alles Unsichtbare" verallgemeinert (§4.3).

## 3. `model` wird nicht getrimmt — offen gelassen, nicht vorentschieden

**Befund (Abschluss-Review):** Ein erster Entwurf des T12-Tests behauptete
per Assertion, ein Modellname mit Randleerzeichen bleibe erhalten, und
begründete das mit „Anzeigetext". Das ist falsch: `model` geht in den
Request-Body jedes Provider-Aufrufs und in `provider_identity_key`. Ein
Randzeichen dort ergibt ein `AI_MODEL_NOT_FOUND` ohne erkennbaren Grund —
genau das Fehlerbild, das diese Spec für Keys beseitigt.

**Entscheidung:** Die Assertion auf `model` ist entfernt, T12 prüft nur
noch `display_name` (das ist auch alles, was die Spec verlangt). Ob `model`
getrimmt gehört — und mit welcher Semantik, denn es ist kein Zugangsdatum —
bleibt offen und ist hier als Frage festgehalten statt durch einen Test
beantwortet.

## 4. Kein `is_empty()`-Wächter in `add_ai_provider` — bewusst nicht behoben

**Befund (Abschluss-Review):** `commands::add_ai_provider` schreibt
`config.api_key` unbedingt in den `CredentialStore`, ohne den
`!api_key.is_empty()`-Wächter, den `update_ai_provider` hat. Ein Key aus
lauter unsichtbaren Zeichen legt damit einen Provider mit leerem Credential
an, ohne Meldung. Die Frontend-Sperre greift nicht: JS-`trim` entfernt zwar
U+FEFF, nicht aber U+200B/200C/200D/2060.

**Entscheidung:** **nicht behoben**, aus zwei Gründen.

Erstens ist der Zustand nicht neu. `add_ai_provider` rief schon vor dieser
Spec `config.trimmed()` auf; ein Key aus lauter Leerzeichen erzeugte bereits
dasselbe leere Credential. Diese Spec erweitert nur die Menge der Eingaben,
die dorthin führen.

Zweitens wäre ein Fehler statt einer stillen Anlage **neues Verhalten bei
leerer Eingabe** — Spec 0073, §2, Nicht-Ziel 3 schließt das ausdrücklich
aus.

Gehört als eigener Punkt ins Backlog: „`add_ai_provider` lehnt einen leeren
`api_key` sichtbar ab, statt einen Provider ohne Credential anzulegen."

## 5. Die Laufzeitschranke in X1 bleibt, die in X2 fällt weg

X1 verlangt ausdrücklich „die Laufzeit bleibt linear". Umgesetzt als eine
Wall-Clock-Schranke von 5 s bei 100 000 Randzeichen: Ein einzelner
Durchlauf liegt im Debug-Build bei ~10 ms, ein quadratisches Abschneiden
läge bei Minuten — dazwischen liegen mehrere Größenordnungen, die Schranke
trennt also zuverlässig und hängt nicht an Laufzeitschwankungen.

X2 verlangt nur „leerer String, keine Panik". Die dort zunächst ebenfalls
eingebaute Zeitschranke ist entfernt: eine Assertion ohne Auftrag, die nur
eine zweite Stelle wäre, an der ein überlasteter Rechner den Lauf rot
färben könnte.

Beide Tests prüfen genau genommen eine Eigenschaft von
`std::str::trim_matches`, nicht von neuem Code. Sie bleiben als Wächter
gegen eine künftige Eigenimplementierung stehen — das ist ihr Zweck, und
er steht hier, damit niemand sie später für überflüssig hält.

## 6. Ein Eigenschaftstest für I3 statt einer Handvoll Beispiele

**Befund (Abschluss-Review):** I3 („kann per Konstruktion nicht weniger
bereinigen als `str::trim`") hielt sachlich, war aber nicht abgesichert:
Alle Tests kamen mit Space, Tab, `\r` und `\n` aus. Ein späteres Ersetzen
von `c.is_whitespace()` durch das naheliegend wirkende
`c.is_ascii_whitespace()` hätte die gesamte Suite grün gelassen und dabei
U+00A0 (geschütztes Leerzeichen — der klassische Web-Copy-Paste), U+3000
und U+2028 verloren.

**Entscheidung:** `test_i3_every_unicode_whitespace_char_is_still_trimmed`
läuft über den gesamten Unicode-Bereich und prüft für jedes `char` mit
`is_whitespace()` sowohl das Ergebnis als auch die Gleichheit mit
`str::trim`. Der Gegenbeweis wurde geführt: Mit
`c.is_ascii_whitespace()` ist dieser Test der **einzige** der 15, der rot
wird.

## 7. Zurückgestellt, nicht vergessen

Vom Abschluss-Review gemeldet, außerhalb des Auftrags dieser Spec, jeweils
als Backlog-Punkt vorzumerken:

- **`extra_headers`-Werte laufen durch keinen Trim.** Sie tragen in der
  Praxis Zugangsdaten und können `authorization`/`x-api-key` sogar
  überschreiben. Spec §1 listet sie nicht, A3 ist damit formal erfüllt.
- **Frontend und Backend meinen Verschiedenes mit „leer".** `apiKeyFormat.ts`,
  `AiProviderSettings.tsx` und `ollama.ts` nutzen JS-`trim`, das
  U+200B/200C/200D/2060 stehen lässt. Folge: Der Präfix-Hinweis feuert bei
  einem Key, den das Backend inzwischen sauber verarbeitet. Ein
  TS-Zwilling des Helfers würde das schließen. Spec §2, Nicht-Ziel 4 zieht
  die Grenze bei „Aufrufer außerhalb dieses Repos" — das Frontend liegt
  *in* diesem Repo, die Umstellung ist aber eigener Scope.
- **Kein Zeroize auf dem Zwischenwert.** `trim_credential_value` gibt einen
  gewöhnlichen `String` zurück, der erst danach in `SecretString` wandert.
  A5 schreibt genau diese Signatur vor; unverändert gegenüber vorher.
- **Ein gewolltes unsichtbares Zeichen am Rand eines Passworts wird jetzt
  still entfernt.** §4.2 begründet das Randtrimmen damit, dass „kein
  Anbieter Schlüssel vergibt, die damit beginnen oder enden" — bei einem
  selbst gewählten Passwort oder einer Passphrase trägt diese Begründung
  weniger weit. Die Spec ordnet es für alle drei Server-Secrets an (T10),
  es ist also entschieden; hier nur festgehalten.
