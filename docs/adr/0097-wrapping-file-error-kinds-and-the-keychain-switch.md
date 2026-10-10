# ADR 0097 — Fehlerarten der Verpackungsdatei und der Wechsel auf den Schlüsselbund

Status: akzeptiert
Betrifft: Spec 0101 (A3, A5, A15–A17, §7, Klarstellung 10 a–d), ADR 0095 §7,
ADR 0096 §5

Klarstellung 10 der Spec hat vier Punkte umzusetzen gegeben (a–d; Punkt e
gehört zu Commit 11). Dieses Dokument hält fest, welche Entscheidungen dabei
offen waren, wie sie gefallen sind, und was bewusst stehen geblieben ist.

## 1. Ein Lesefehler ist kein Urteil über den Inhalt (10a)

Die Verpackungsdatei wird jetzt wie der Schlüsselbund in A3 nach **Fehlerart**
eingeordnet:

| Zustand | A3-Spalte | „Neu anfangen“ |
|---|---|---|
| nichts an diesem Ort | — (Schlüsselbund-Modus) | — |
| gelesen, Kopf/Parameter in Ordnung | K nach dem Entsperren *da* | nein |
| gelesen, Kopf/Version/KDF/Parameter/Länge falsch | *ungültig* (D3/D4) | **ja** |
| nicht lesbar: Rechte, E/A, von einem Programm gehalten, Verknüpfung ins Leere | *nicht erreichbar* (D1) | nein |

Vorher war die letzte Zeile *ungültig*. Die Folge war der Fund aus der
vorigen Review-Runde: `chmod 000` auf eine **vollkommen gültige** Verpackung
bot „Neu anfangen" an, und die Annahme benannte Datenbank, `-wal`, `-shm`
**und** die intakte Verpackung um und erzeugte einen neuen K. Der
Datenbestand war damit nicht verloren (umbenannt, nicht gelöscht), aber der
Verlauf hing an einem Schlüssel, den niemand mehr brauchte.

**Zwei Entscheidungen, die dabei zu treffen waren:**

- **Die hängende Verknüpfung zählt jetzt als *nicht erreichbar*.** ADR 0096
  §5 hatte vorgeschlagen, sie als einzigen Lesefehler beim Ausweg zu lassen
  (ihr Ziel gibt es wirklich nicht). Der Wortlaut von 10a sagt aber „Fehlt
  die Datei **oder lässt sie sich nicht lesen** … gilt sie als *nicht
  erreichbar*", und *ungültig* ausdrücklich nur für einen **gelesenen**
  Inhalt. Die Spec entscheidet das, nicht der Vorschlag im ADR. Der Preis
  steht in Abschnitt 4.
- **`read_wrapping_file` fragt dieselbe Frage wie `key_mode`.** Ein `read`
  auf eine Verknüpfung ins Leere liefert `NotFound` — als „es ist kein
  Master-Passwort eingerichtet" gelesen widersprach das der Modusbestimmung,
  die für denselben Zustand `Password` sagt. Jetzt heißt `NotFound` nur dann
  „kein Passwort-Modus", wenn auch `symlink_metadata` nichts findet.

Dazu ein eigener Fehler `WrappingFileUnreachable`: Sein Text sagt „nicht
lesbar, es wurde nichts verändert, versuche es erneut" statt „ließ sich nicht
schreiben". Der Fehlercode zur Oberfläche bleibt
`MASTER_PASSWORD_FILE_FAILED` — ein eigener Code wäre ein Orakel darüber, ob
die Datei existiert und bloß gesperrt ist, und die Oberfläche tut in beiden
Fällen dasselbe.

## 2. Der Wechsel auf den Schlüsselbund fragt, bevor er überschreibt (10b)

Es gibt genau **einen** Platz je Benutzer für K im Schlüsselbund. Lag dort
schon ein anderer Schlüssel, überschrieb ihn der Wechsel ohne Lesen, ohne
Warnung, ohne Frage. Die Lage, in der das weh tut: Installation A im
Schlüsselbund-Modus, Datenverzeichnis B im Passwort-Modus, B wechselt zurück
— As Schlüssel ist weg und As Datenbank beim nächsten Start nicht mehr zu
öffnen.

`switch_to_keychain` liest deshalb **immer** vorher:

- Eintrag gleich K oder kein Eintrag → kein Wort, weiter wie bisher.
- Ein anderer oder ein unbrauchbarer Eintrag ohne Bestätigung → Abbruch mit
  `KEYCHAIN_HOLDS_ANOTHER_KEY`, **nichts** angefasst (kein `set`, kein
  `delete`, Verpackung bleibt und gibt weiter K her).
- Mit Bestätigung → eine eigene `warn`-Zeile, die benennt, dass ein fremder
  Wurzelschlüssel unwiederbringlich ersetzt wurde. Ohne sie sähe der einzige
  unumkehrbare Schritt des Vorgangs im Log wie der Normalfall aus.
- Schlüsselbund nicht erreichbar → Abbruch **vor** dem `set`, auch mit
  Bestätigung: Bestätigt wurde das Ersetzen eines bekannten Eintrags, nicht
  ein Schreiben ins Ungewisse. (Vorher scheiterte der Wechsel erst beim
  Zurücklesen — also nachdem er geschrieben hatte.)

Ein unbrauchbarer Eintrag zählt als „nicht gleich K": Wessen Eintrag dort
liegt, ist von hier aus nicht zu erkennen, und A17 behandelt denselben
Zustand beim Entsperren genauso (nichts gelöscht).

**Nicht umgesetzt:** eine Nachprüfung, dass beim bestätigenden zweiten Aufruf
noch **derselbe** fremde Schlüssel dort liegt. Sie wäre strenger, braucht
aber einen zweiten Dialogtext für den Fall „inzwischen liegt dort ein
anderer" — und damit den Dialog, der erst in Commit 11 entsteht. Bis dahin
ist der Weg über `replace_another_key` nur aus der Oberfläche erreichbar und
immer mit der Log-Zeile verbunden.

## 3. Wie 10c und 10d belegt sind

- **10c, Plugin-Aufschiebung:** zwei quelltextlesende Tests nach demselben
  Maßstab wie `test_t18_the_gate_is_wired_…` (ADR 0096 §3 begründet, warum
  diese Form hier die einzig mögliche ist): Die fünf Plugins stehen in
  `lib.rs` an genau zwei Stellen, und außerhalb dieser beiden Funktionen wird
  nur das Plugin registriert, das den Fensterrahmen gestaltet; aufgeschoben
  wird genau dann, wenn beim Start kein Zustand steht. Beide gegen drei
  gefährliche Veränderungen rot gesehen.
- **10d, T17 auf allen Pfaden aus §7:** Die Suchbegriffe — jede Schreibweise
  von K, der Datenbankschlüssel, Passwörter, Secrets, Marker — stehen jetzt
  an einer Stelle (`test_support::key_leak_needles`) und werden von allen
  T17-Tests benutzt. Neu dazu: der Pfad T3 (Entscheidungstabelle samt
  Fehlerfeldern und „Neu anfangen", einschließlich der **erzeugten**
  Schlüssel, die aus dem Test-Schlüsselbund zurückgelesen werden) und der
  Pfad T11 (Secret-Umzug mit scheiterndem `get` und `delete`). Der Aufbau
  beider Tests ist zugleich die Lage aus T1: frische verschlüsselte
  Datenbank, Server, Provider, Secrets, offener Pool.

## 4. Was offen bleibt

1. **Dauerhaft gescheiterte Authentifizierung hat keinen Ausweg in der App.**
   10a sagt im letzten Satz: „Dauerhaft gescheiterte Authentifizierung bleibt
   ‚Passwort falsch oder Datei beschädigt‘ (A17) mit ‚Neu anfangen‘ als
   ausdrücklicher Wahl." Der Riegel aus ADR 0095 §7 lehnt aber jede Datei mit
   brauchbarem Kopf ab — und nach A17 sind „ein Byte im Chiffrat gekippt" und
   „Passwort vergessen" nicht unterscheidbar. Wer sein Passwort vergisst,
   kommt in der App nicht weiter; wessen Datei beschädigt ist, auch nicht.
   Beides gilt bis zu einer Entscheidung, denn sie zu beheben heißt, den
   Riegel zu lockern, der genau diesen Knopf vor dem vergessenen Passwort
   schützt. **Das ist eine Produktentscheidung** (Schutz vor Datenverlust
   gegen dauerhafte Aussperrung) und liegt als Frage vor, nicht als Annahme.
   Dasselbe gilt für die hängende Verknüpfung und einen dauerhaften
   E/A-Fehler: „Erneut versuchen" gelingt dort nie.
2. **Der dritte Zustand fehlt im DTO.** `StartupStateDto` trägt ein
   `wrapping_unusable: bool`; *nicht erreichbar* ist darin nicht abbildbar
   und führt heute zu `needs_unlock = true`. Der Nutzer sieht also das
   Passwortfeld und bekommt beim Abschicken die ehrliche Meldung „nicht
   lesbar, nichts verändert, versuche es erneut" — D1 in der Sache, aber
   nicht in der Form. Ein dritter Zustand gehört zu Commit 11, wo die Maske
   entsteht, und ist dort Pflicht, sonst ist A3 Zeile *sonst* × *nicht
   erreichbar* nicht darstellbar.
3. **Zwei Nachbesserungen der vorigen Runde haben weiter keinen Test:** der
   Aufruf von `spawn_post_startup_tasks` aus dem Entsperrpfad und das
   Zurücksetzen der Timeout-Kennzeichnung beim Öffnen jeder Frage. Beide
   wären prüfbar (der erste quelltextlesend, der zweite über zwei Fragen auf
   demselben Kanal). Sie fehlen aus Budgetgründen dieses Lauf­s, nicht weil
   sie verzichtbar wären — `CLAUDE.md` verlangt zu jedem Fix den Test, der
   ihn hält.
4. **Weiter zurückgestellt**, unverändert begründet in ADR 0096 §5: ein
   reiner Hinweis wartet fünf Minuten und der Passwortplatz wird beim Öffnen
   der Frage nicht geleert (beides Klarstellung 10e, Commit 11) ·
   A19 im Bestand. **Erledigt mit #266:** `create_new` für die `.new`-Datei
   der Verpackung (eine liegengebliebene reguläre Datei wird zuvor entfernt,
   eine Verknüpfung lässt das Schreiben scheitern) · die Argon2-Obergrenze
   (256 MiB) · der Hinweis an `Wiring::plugins`.
5. **Zwei Härtungen aus der Review-Runde, klein und unabhängig:**
   `key_mode`/`wrapping_health` werten die Fehlerart von `symlink_metadata`
   nicht aus (heute nicht ausnutzbar, weil die Dateizustandsprüfung vorher
   abbricht; **erledigt mit #266:** nur `NotFound` heißt „keine Datei“, jeder
   andere Fehler ergibt `Password` bzw. `Unreachable`), und T17 fährt den D2-Pfad nicht, auf dem ein SQL-Fehlertext den
   `PRAGMA`-Wert tragen könnte (dafür gibt es eigene Tests in
   `persistence-sqlite`).
6. **`KEYCHAIN_HOLDS_ANOTHER_KEY` steht noch nicht in der Codeliste des
   Frontends.** Das Frontend ruft `switch_to_os_keychain` heute nicht auf;
   Code, Text und Dialog gehören zusammen und entstehen in Commit 11.
