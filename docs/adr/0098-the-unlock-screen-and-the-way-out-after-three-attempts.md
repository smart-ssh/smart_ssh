# ADR 0098 — Die Entsperrmaske, der Ausweg nach drei Versuchen und was dabei offen blieb

Status: akzeptiert
Betrifft: Spec 0101 (A3, A5, A13, A16–A18, A20, §7 T14, Klarstellungen 9,
10a/10b/10e, 11), ADR 0095 §7/§8, ADR 0096 §5, ADR 0097 §4

Commit 11 der Spec baut die Oberfläche zur Etappe 3. Dieses Dokument hält
fest, welche Entscheidungen dabei offen waren, wie sie gefallen sind, und
was bewusst stehen geblieben ist.

## 1. Ein Feld statt drei Flaggen (ADR 0097 §4, Punkt 2)

`StartupStateDto` trug `unlocked`, `needs_unlock` und `wrapping_unusable` —
drei Wahrheitswerte, aus denen die Oberfläche einen Zustand zusammensetzen
musste. Einer der vier Zustände aus A3 war darin **nicht abbildbar**: *nicht
erreichbar* führte zu `needs_unlock = true`, der Nutzer sah also ein
Passwortfeld für eine Datei, die gar nicht gelesen werden konnte.

Jetzt trägt das DTO ein `screen`-Feld mit fünf Werten (`unlocked`,
`unlock`, `unusableWrapping`, `unreachableWrapping`, `setUpMasterPassword`).
Ein Feld mit fünf Werten kann sich nicht selbst widersprechen.

Dazu: **Modus und Dateizustand kommen aus einer Lesung.** `key_mode` und
`wrapping_health` stellen dieselbe Frage an dieselbe Datei; getrennt
gefragt könnten sie sich widersprechen, wenn die Datei dazwischen
verschwindet — und der Widerspruch landete in der Maske.

## 2. Der Ausweg nach drei Fehlversuchen (Klarstellung 11, Q-BL-0314-01)

Die Frage war entschieden (K3, Option 2 mit einem Zähler nur im Speicher).
Umgesetzt mit vier Einschränkungen, die zusammen tragen:

- **Gezählt wird nur eine gescheiterte Authentifizierung.** Ein Datei- oder
  Schlüsselbundfehler sagt nichts darüber, ob das Passwort passt; ihn
  mitzuzählen hieße, den Ausweg über eine dreimal nicht lesbare, vielleicht
  vollkommen intakte Datei freizuschalten.
- **Der Zähler liegt nur im Speicher** (`AtomicU32` in `PendingStartup`) und
  beginnt bei jedem Start bei null. Ein dauerhafter Zähler wäre eine neue
  Datensenke neben der Datenbank und zugleich ein Weg, den Ausweg durch
  Warten statt durch Versuche freizuschalten.
- **Eine *nicht erreichbare* oder fehlende Datei bietet ihn nie**, auch
  nicht nach beliebig vielen Versuchen.
- **Maske und Kommando entscheiden dieselbe Frage mit derselben Funktion**
  (`decide_startup_screen`). Zwei Fassungen davon wären zwei Gelegenheiten,
  sie auseinanderlaufen zu lassen — und die gefährliche Richtung ist die, in
  der das Kommando mehr annimmt als die Maske anbietet.

**Die alte Prüfung bleibt wörtlich stehen.** `decide_startup_screen` fragt
`WrappingHealth::allows_starting_over()` und verknüpft das Ergebnis mit der
neuen Bedingung per ODER. Damit kann die neue Fassung per Konstruktion nicht
weniger ablehnen als die alte; sie kommt nur in der einen, entschiedenen
Lage dazu. (Dass dieser Aufruf fehlte und der dokumentierte Riegel damit nur
noch Testcode war, ist ein Fund der Review-Runde zu diesem Commit.)

**Nachtrag zu ADR 0095 §7:** Dort steht „Bei einer brauchbaren Datei wird
abgelehnt". Das gilt unverändert bis zum dritten Fehlversuch desselben
Programmlaufs; danach gilt die Ausnahme aus Klarstellung 11, beschrieben
hier.

**Nachtrag zu ADR 0097 §4, Punkt 1:** Die dort als offen geführte Frage
(„Dauerhaft gescheiterte Authentifizierung hat keinen Ausweg in der App")
ist entschieden und umgesetzt. Punkte 2 und 3 derselben Liste sind mit
diesem Commit erledigt; Punkt 6 (`KEYCHAIN_HOLDS_ANOTHER_KEY` in der
Codeliste des Frontends) ebenso.

## 3. Ein Hinweis wartet auf keine Antwort (Klarstellung 10e)

Der Hinweis aus A5 („die Dateien heißen jetzt …") lief durch dieselbe
Wartefunktion wie eine echte Frage: Er hielt den Startablauf auf, bis jemand
„OK" drückte, und lief nach fünf Minuten in die Zeitgrenze — mit dem
Ergebnis, dass der Start danach als „keine Antwort erhalten" abbrach,
**obwohl das Umbenennen schon passiert war**.

Jetzt zeigt `show_only` den Hinweis und kehrt zurück, ohne eine Frage offen
zu lassen. **Der Dateiname geht dabei nicht verloren:** Die Oberfläche hält
den Hinweis in eigenem Zustand und zeigt ihn auch nach der Entsperrung
weiter, bis der Nutzer ihn wegklickt. Dieses Wegklicken schickt
**nichts** ans Backend — eine Antwort ohne Frage könnte die nächste, echte
Frage vorab beantworten.

Die zweite Hälfte von 10e („ein geöffnetes Passwortfeld ist leer") hängt an
zwei Stellen: Die Maske bekommt je Frage einen neuen Zustand, und das
Backend leert seinen Platz für das neue Passwort, **bevor** es fragt. Ohne
Letzteres genügte ein `answer_startup_prompt` mit `Confirm` und ohne
Passwort, um ein Passwort aus einem früheren, abgebrochenen Versuch zu
bestätigen.

## 4. Der dritte Knopf in D1 (A3, A13)

Er war zurückgestellt, solange es die Maske im Fenster nicht gab. Jetzt
erscheint er in beiden Dialogarten — nativ und im Fenster — und ist dreifach
eingeschränkt: Die Bedingung aus A3/D1 liegt unverändert in
`password_setup_is_safe`, das Fenster hat dafür eine **eigene** Dialogart
(`RetrySetUpOrQuit`, kein Zusatzfeld an einer bestehenden), und eine über
IPC erfundene Antwort wird je Dialogart in „beenden" übersetzt. Der
Umzugs-Dialog aus A11 kann ihn per Konstruktion nicht anbieten (T13:
„Umzugs-Dialog (A11) nie").

## 5. Fehlertexte, die ihren Zustand treffen (Klarstellung 9, Punkt 5)

Die Oberfläche übersetzt den Fehlercode und zeigt den Text des Backends
nicht — anders wäre ein deutscher Satz im englischen Fenster unvermeidlich
(Spec 0024, Abschnitt 5). Das heißt aber: **Jeder Code muss einen Text
tragen, der in allen seinen Fällen stimmt.** Unter
`MASTER_PASSWORD_FILE_FAILED` lagen auch der Schlüsselbund-Fehlabgleich und
ein Vorgang im falschen Modus; der Nutzer las dort „Die Schlüsseldatei neben
deiner Datenbank ließ sich nicht lesen", obwohl es im Schlüsselbund-Modus
gar keine gibt.

Zwei eigene Codes (`KEYCHAIN_KEY_MISMATCH`,
`MASTER_PASSWORD_MODE_MISMATCH`), und der gemeinsame Text nennt keinen Ort
mehr, den es nicht geben muss. **Kein Orakel:** Beide sagen nichts über die
Existenz einer Verpackungsdatei, nur etwas über den Schlüsselbund und den
Modus — und den zeigen die Einstellungen ohnehin an (A18). Die Entscheidung
aus ADR 0097 §1, *nicht erreichbar* und *nicht schreibbar* unter einem Code
zu halten, bleibt davon unberührt.

## 6. Was offen bleibt

1. ~~**Die ausdrückliche Bestätigung der Warnung aus A13/E10 liegt nur in
   der Oberfläche.**~~ **Erledigt** — entschieden als Klarstellung 12
   (Q-BL-0314-02): Das Backend prüft die Bestätigung wie Länge und
   Wiederholung, und jeder Einrichtungsweg lehnt ohne sie ab. Umgesetzt in
   ADR 0099; die Begründung „eine Prüfung, die nur dort steht, umgeht ein
   Kommandoaufruf" gilt damit für alle drei Zusagen aus A13.
2. **Die Mindestlänge steht an drei Stellen.** `key_wrapping` ist
   maßgeblich, die beiden Masken wiederholen sie. Sie zählen jetzt
   gleich (Codepunkte, `characterCount`), aber die Zahl selbst ist dreimal
   geschrieben. Ein Kommando, das sie liefert, wäre besser und ist
   zurückgestellt.
3. **Eine Frage, die nach der Entsperrung einträfe, wird nicht angezeigt.**
   Heute nicht erreichbar — alle Fragen liegen innerhalb des Aufbaus, und
   das Tor öffnet danach. Entsteht je ein Dialog nach dem Öffnen des Tors,
   ist das eine stille Senke.
4. **Keine Fokusfalle und kein `role="dialog"`** für die Startfragen (Spec
   0091 hat das für den Host-Key-Dialog etabliert). Hinweis und Frage können
   zugleich stehen und überlagern sich dann.
5. **`get_startup_state` liest je Aufruf die ganze Verpackungsdatei.** Vor
   der Entsperrung ist das die einzige unbegrenzt aufrufbare Datei-E/A. Nur
   Last, kein Zustandswechsel, keine Datei wird verändert.
6. **Weiter zurückgestellt**, unverändert begründet in ADR 0096 §5 und
   ADR 0097 §4: `create_new` für die `.new`-Datei der Verpackung · die
   Argon2-Obergrenze · ein Hinweis an `Wiring::plugins` · A19 im Bestand ·
   die Fehlerart von `symlink_metadata` in `key_mode`/`wrapping_health` ·
   T17 auf dem D2-Pfad · zwei Quellen für `default_db_path` · der neue
   Dateiname im D4-Dialog.
7. **Die zweite Review-Runde zu diesem Commit ist nicht gefahren.** Nach
   Runde 1 und den Nachbesserungen war das Budget zu knapp, um sie zu
   beginnen. Die Nachbesserungen sind entweder strikt strenger (die alte
   Prüfung wörtlich ODER-verknüpft, Zeichen statt UTF-16-Einheiten, der
   Zuhörer nachweislich vor dem Kommando) oder reine Ergänzungen (zwei
   Fehlercodes mit Texten, zwei Knöpfe, vier Tests) — geprüft von außen sind
   sie nicht.
