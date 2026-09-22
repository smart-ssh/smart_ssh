# 0061-keychain-unavailable-reporting-decisions

## Status

Proposed — gehört zu Spec 0071.

## Kontext

Spec 0071 lässt an mehreren Stellen offen, *wie* etwas umgesetzt wird, und
eine Vorgabe (Teil 0) wurde während der Umsetzung geändert. Dieses ADR hält
die Stellen fest, an denen bei der Umsetzung eine Wahl getroffen wurde, die
die Spec nicht wörtlich vorgibt — damit ein späterer Leser nicht
rekonstruieren muss, ob das Absicht war.

## Entscheidungen

### 1. Die Paketnamen — gemessen, und die erste Annahme war teils falsch

Spec 0071 §3 A0.3 verlangt die Paketnamen „nachgewiesen durch `apt install`
und einen anschließend erfolgreichen Provider-Anlegeversuch, nicht aus dem
Gedächtnis". Bei der Umsetzung stand keine Linux-Umgebung zur Verfügung;
die Namen gingen deshalb zunächst als markierte Annahme (`ANNAHME A-1`) in
den Code. **Die Messung ist inzwischen nachgeholt** (Debian 13 „trixie",
Container; Zahlen und Fehlerketten in §9 der Spec), die Annahme ist
aufgelöst — und sie war an zwei Stellen falsch.

**Befund:**

| Bisher im Text | Debian 13 | Ergebnis |
|---|---|---|
| `gnome-keyring` | 48.0-1 | **richtig und ausreichend.** Einziges Paket im Archiv mit `/usr/share/dbus-1/services/org.freedesktop.secrets.service`, startet also per D-Bus-Aktivierung von selbst. `libpam-gnome-keyring` ist **nicht** nötig (nur fürs automatische Entsperren beim Login) — mein Zweifel im Bericht ging hier fehl. |
| `dbus-user-session` | 1.16.2-2 | richtig für den fehlenden Session-Bus. |
| `kwalletd6` | **existiert nicht** | Der Daemon heißt `kwallet6` und registriert `org.kde.kwalletd5`/`…6`, nicht `org.freedesktop.secrets`. Installieren behebt die Lage dort also nicht. |
| KeePassXC | 2.7.10 | Paket existiert, bringt aber keine D-Bus-Dienstdatei mit: Der Name wird erst angemeldet, wenn die Anwendung läuft **und** die Secret-Service-Integration eingeschaltet ist (Vorgabe: aus). |

**Entscheidung:** Der Text stellt die drei nicht länger als gleichwertige
Alternativen nebeneinander, weil sie es nachweislich nicht sind.

- **Genau ein** Installationsbefehl: `sudo apt install gnome-keyring` —
  ausdrücklich auch für KDE, weil er dort nachweislich funktioniert.
- KWallet (`kwallet6`) und KeePassXC stehen in einem zweiten Satz als
  „falls ohnehin in Gebrauch", mit dem Hinweis, dass ihre
  Secret-Service-Integration laufen bzw. eingeschaltet sein muss.
  **Kein** `apt install` für die beiden.

Ein Test hält das fest: Der Anbieter-Text darf `apt install` genau
**einmal** enthalten, und zwar für `gnome-keyring`. Ohne diese Grenze
könnte jemand den zweiten Satz wieder zu einem Installationsvorschlag
ausbauen, der die Lage nicht behebt.

**Weiterhin offen, nur an einer echten KDE-Sitzung zu klären (M4):** ob
eine vollständige Plasma-Installation den Secret Service doch über eine
andere Komponente bereitstellt. Bis dahin nennt der Text auch für KDE
`gnome-keyring` als den Weg, der belegt funktioniert.

### 2. Der Schlüsselbund-Zustand darf sich nachträglich verschärfen

A16 verlangt, den Zustand einmal pro Programmlauf zu ermitteln und im
`AppState` zu halten, damit kein Kommando zusätzlich probiert. Der Review
zeigte, dass ein reiner Schnappschuss zu wenig ist: `store_status()` kann
`Ok(())` melden (der Anbieter antwortet auf den Verbindungsaufbau), und der
erste echte Zugriff scheitert trotzdem — der Normalfall bei einem
vorhandenen, aber gesperrten Schlüsselbund.

**Entscheidung:** `escalate_to_unavailable` verschärft den Zustand, wenn
`resolve_or_generate_key` beim Start mit `KeyStoreAccessFailed` scheitert.
**Nur** in Richtung „nicht verfügbar"; ein bereits klassifizierter, weil
spezifischerer Grund wird nie überschrieben, und es gibt keinen Weg zurück
auf `Available`.

**Begründung:** Das ist kein zusätzliches Probieren im Sinne von A16 — die
Information fällt auf dem bestehenden Startpfad ohnehin an. Ohne die
Eskalation liefen A13 und A15 im wichtigsten Fall (gesperrter
Schlüsselbund) ins Leere.

**Konsequenz 1:** Sperrt sich der Schlüsselbund erst *nach* dem Start
(Bildschirmsperre nach Zeitablauf), bleibt der Zustand weiterhin stale.
Siehe offener Punkt 1 unten.

**Konsequenz 2 (spec-reviewer-Fund, 2. Runde — ausdrücklich benannt):**
`CipherError::KeyStoreAccessFailed` entsteht aus **jedem**
`CredentialError::Backend` auf dem App-Schlüssel-Slot. Auf macOS genügt
damit ein einziges „Nicht erlauben" im Keychain-Dialog beim Start, um den
ganzen Programmlauf auf `Unavailable(Unknown)` zu setzen. Folge: Die
Diagnose-Zeile meldet „nicht verfügbar — Ursache unbekannt", obwohl der
Schlüsselbund grundsätzlich funktioniert, und spätere Backend-Fehler
zeigen den generischen Text statt der spezifischen OS-Meldung. Die
Richtung ist konservativ (nie ein falsches „verfügbar"), aber M5 verlangt
auf macOS „unverändertes Verhalten" — **das ist beim manuellen Test M5
ausdrücklich mitzuprüfen**, und falls es stört, ist die Eskalation auf
Linux einzugrenzen.

### 2a. Vom Nutzer ausgelöstes Entfernen: scheitern vs. melden (A17)

Der Review dieser Spec zeigte, dass §6.3 X6 zwei verschiedene Dinge in
einen Topf warf: echte Aufräumpfade (ein `delete`, das einen bereits
gescheiterten Vorgang zurückbaut) und **vom Nutzer angeforderte**
Entfernen-Aktionen. Bei letzteren meldete ein verschluckter Fehler Erfolg,
während das Secret im Schlüsselbund stehen blieb. Die Spec wurde
daraufhin korrigiert (A17 ist neu, X6 trägt die Korrektur).

**Entscheidung (2026-09-22), zwei verschiedene Antworten für zwei
verschiedene Lagen:**

- **„Hinterlegtes Sudo-Passwort entfernen"** schlägt **sichtbar fehl**
  (Fehlerweg wie A13). Begründung: Es ist sonst nichts geschehen — es gibt
  keinen Teilerfolg, den man melden könnte. Ein `Ok(())` wäre unwahr, und
  der Nutzer sähe „kein Sudo-Passwort hinterlegt", während das Passwort
  beim nächsten `sudo` weiter eingespeist würde.
- **Server löschen** läuft **durch**. Begründung: Hier gibt es einen
  echten Teilerfolg (das Profil ist weg), und niemand soll auf einem
  unlöschbaren Server sitzen bleiben, nur weil der Schlüsselbund klemmt.
  Das Ergebnis trägt dafür `secrets_left_behind` — die `CredentialRef`s,
  die im Schlüsselbund blieben. Sie sind danach verwaist (die Server-ID
  existiert nicht mehr), deshalb nennt die Oberfläche sie beim Namen: ohne
  den Ref findet der Nutzer den Eintrag im Schlüsselbund nicht wieder. Ein
  `CredentialRef` ist kein Secret, nur der Account-Name innerhalb des
  Service „Smart SSH".

**Konsequenz:** Zwei getrennte Funktionen (`clear_sudo_password` und
`delete_sudo_password_on_server_delete`) statt eines Schalters — die
beiden Hälften von A17 sollen sich nicht über einen Parameter vermischen
lassen. Die `tracing::warn!`-Zeilen aus der vorigen Runde bleiben in
beiden Fällen.

**Abweichung von Entscheidung 4, bewusst:** `clear_sudo_password` hängt
`KEYCHAIN_UNAVAILABLE` **unbedingt** an, nicht abhängig vom
Schlüsselbund-Zustand aus dem `AppState`. Grund (spec-reviewer-Fund):
Dieser Fehlerpfad entsteht durch A17 überhaupt erst; wäre er an den
Startzustand gekoppelt, stünde dort der rohe englische Bibliothekstext,
sobald der Schlüsselbund erst *während* der Sitzung klemmt — also genau
das, was §2 ausschließt. Sachlich ist die unbedingte Zuordnung hier auch
richtig: Ein `Backend`-Fehler auf einem `delete` hat keine andere Ursache
als einen Schlüsselbund, der nicht tut, was er soll. Der allgemeine
Fall (offener Punkt 1 unten) bleibt davon unberührt.

**Der generische Code-Text reicht auf diesem Pfad nicht:** Er spricht von
„speichern oder lesen" und sagt damit nicht, worauf es hier ankommt — dass
das Passwort weiter im Schlüsselbund liegt und beim nächsten `sudo` erneut
eingespeist wird. Das Formular stellt deshalb einen eigenen Satz voran
(`serverForm.removeSudoPasswordFailed`), statt dafür einen zweiten
Fehlercode einzuführen. Dasselbe Muster nutzt die Diagnose-Ansicht bereits
für ihre Fehlermeldungen.

### 3. Außerhalb von Linux gilt immer der neutrale Text

A8 verlangt, dass auf macOS/Windows kein Linux-Paketname, kein `apt`-Befehl
und kein „Secret Service" erscheint. Der Klassifizierer liefert dort ohnehin
nur `Unknown` (A1-Tabelle, T5).

**Entscheidung:** `keychain_unavailable_text` bildet außerhalb von Linux
*jeden* Grund auf den neutralen Text ab, nicht nur `Unknown`.

**Begründung:** A8 wird damit per Konstruktion wahr statt nur per
Aufrufkonvention.

**Konsequenz:** Ein Aufruf mit widersprüchlichen Parametern scheitert nicht
sichtbar, sondern liefert den (nach A4 vollständigen, aber allgemeineren)
Auffangtext. In der Diagnose-Ansicht gilt das **nicht** per Konstruktion:
Das Frontend rendert den vom Backend gelieferten Grund über den
Übersetzungskatalog und kennt das Ziel-Betriebssystem nicht. Dort schützt
weiterhin nur der Klassifizierer.

### 4. `KEYCHAIN_UNAVAILABLE` trägt einen allgemeinen, nicht grund-spezifischen Text

A13 schreibt genau **einen** stabilen Code vor. Das Frontend-Mapping
(`errorCodes.ts`, Spec 0024 §5) übersetzt einen Code 1:1 auf **einen**
Schlüssel — ein grund-spezifischer Fehlertext bräuchte entweder vier Codes
(gegen A13) oder den Grund im `message`-String (gegen X2).

**Entscheidung:** Der Text zu `KEYCHAIN_UNAVAILABLE` nennt den Zustand und
die blockierten Funktionen und verweist für Grund und nächsten Schritt auf
die Diagnose-Ansicht (A15).

**Konsequenz:** Wer den Startdialog weggeklickt hat, sieht den Paketnamen
nicht im Fehler selbst, sondern erst nach einem Klick in die Diagnose.
Dafür bleibt X2 („der Code ersetzt den Text") strikt eingehalten.

### 5. Der Schlüsselbund-Zustand steht **nicht** im exportierbaren Diagnosepaket

A15 verlangt die Zeile in der Diagnose-**Ansicht**. Das exportierbare Paket
(Spec 0063) ist ein eigenes, sicherheitsrelevantes Artefakt mit einer
fail-closed-Positivliste.

**Entscheidung:** Nur die Ansicht, nicht das Paket. Ebenso steht die neue
Logzeile mit der vollen `store_status()`-Fehlerkette bewusst **nicht** in
`diagnostics::SAFE_LOG_MESSAGES` und fliegt damit aus dem Export.

**Konsequenz:** Ein an den Support geschicktes Paket verrät nicht, ob der
Schlüsselbund erreichbar war. Kandidat für ein eigenes, kleines Vorhaben.

### 6. §7 Schritte 4 und 5 sind ein Commit

Schritt 4 (Texte) ändert die Signaturen von `db_connect_failure_text`,
`host_key_store_failure_text` und `keychain_unavailable_text`. Deren
Aufrufstellen in `lib.rs` (Schritt 5) müssen im selben Commit mitgehen,
sonst ist das Test-Gate rot. Die Alternative — vorübergehend doppelte
Funktionen mit `#[allow(dead_code)]` — wäre mehr Rauschen als Nutzen
gewesen. Die Schritt-Grenzen 2, 3, 6, 7 und 8 sind unverändert einzeln
prüfbar.

### 7. Das Session-Bus-Indiz wird nur zu einem `bool` verdichtet

A3 nennt das Indiz eine Heuristik, die ausschließlich die Wortwahl steuern
darf; X1 verlangt, dass ein Steuerzeichen in `DBUS_SESSION_BUS_ADDRESS` den
Dialogtext nicht optisch fortsetzen kann.

**Entscheidung:** Der Variablenwert verlässt `lib.rs` nie.

**Messung nachgetragen (A0.4, 2026-09-22):** `store_status()` hängt
nicht. Erstaufruf 3,7 ms (kein Session-Bus), 19 ms (kein Anbieter), 38 ms
(Anbieter vorhanden); jeder weitere Aufruf 167 ns bis 3 µs, weil `keyring`
das Ergebnis in einem `LazyLock` hält. Weit unter der 2-s-Schwelle aus
A0.4 — die Sorge, ein toter Bus-Socket könnte den Start blockieren, hat
sich nicht bestätigt. **Nicht gemessen** ist ein *gesperrter* Anbieter
(braucht eine Sitzung mit Anzeige); das bleibt bei M4.

**§4.3 ist belegt, nicht mehr nur plausibel:** Beide Linux-Fehlerlagen
liefern denselben `keyring::Error`-Zweig (`PlatformFailure`) — „kein
Session-Bus" als `Zbus(Connection(NotFound, …))`, „Bus ohne Anbieter" als
`Zbus(MethodError(ServiceUnknown, …))`. Am `keyring`-API sind sie nicht
unterscheidbar; das Umgebungsindiz aus A3 trennt sie korrekt. Die
Entscheidung gegen einen String-Vergleich auf fremde Fehlertexte war
richtig: Die beiden Texte hätten sich zwar unterschieden, aber genau so
ein Vergleich bricht beim nächsten Versionssprung still.

**Konsequenz:** X1 ist per Konstruktion erfüllt und braucht keine
Bereinigung analog `sanitize_path_for_display` — in **Texten** taucht der
Wert nirgends auf. Im Log kann er dagegen sehr wohl stehen: Die neue Zeile
mit der `store_status()`-Fehlerkette (s. Entscheidung 5) enthält auf Linux
häufig die D-Bus-Adresse samt Socket-Pfad und damit den Benutzernamen. Das
ist dieselbe Kategorie wie die ohnehin geloggten Datenpfade, bleibt lokal
und fliegt aus dem exportierbaren Diagnosepaket; ein `\n` darin kann keine
zweite Logzeile vortäuschen, weil der Logger JSON schreibt und `serde_json`
escaped.

## Bewusst nicht behoben

Diese Punkte kamen im Review auf, wurden geprüft und **nicht** im Rahmen
dieser Spec geändert — jeweils mit Begründung, damit sie nicht
stillschweigend verschwinden.

1. **Ein nach dem Start gesperrter Schlüsselbund bleibt unerkannt.**
   Entscheidung 2 deckt den Zustand beim Start ab. Sperrt sich der
   Schlüsselbund später, meldet ein `CredentialError::Backend` weiterhin
   `code: None` und damit den rohen englischen Text. Ihn *immer*
   anzuhängen wäre möglich (der Text ist generisch und schickt niemanden
   zu `apt install`), würde aber auch einen einzelnen verweigerten Eintrag
   — etwa einen abgebrochenen macOS-Keychain-Dialog — als „Schlüsselbund
   nicht verfügbar" darstellen. Das ist eine Produktabwägung, die A13
   („aus einem nicht verfügbaren Store") nicht entscheidet; sie gehört
   vorgelegt, nicht nebenbei getroffen.

2. **Ein nicht lesbares Sudo-Passwort schaltet still eine Redaktionsschicht
   ab.** `commands.rs` macht an zwei Stellen aus jedem `CredentialError`
   ein `None`. Der `sudo -S`-Pfad läuft dann ohne Passwort weiter (fail-safe),
   aber der Redactor bekommt das Sudo-Passwort-Muster nicht — obwohl der
   Kommentar daneben begründet, warum dieses Muster sicherheitsrelevant
   ist. Praktisch begrenzt (ohne lesbaren Schlüsselbund gelangt das
   Passwort auch nicht neu auf den Server), aber es ist eine stille
   Abschwächung und gehört mindestens geloggt. Außerhalb des Umfangs
   dieser Spec.

3. **`risk_second_opinion.rs` verschluckt den Credential-Fehler mit
   `.ok()?`.** Fail-safe im Sinne von ADR 0024 (die Zweitmeinung kann nur
   eskalieren, ihr Ausfall senkt keine Einstufung), aber unsichtbar: Ohne
   Schlüsselbund ist sie für die ganze Sitzung stumm aus. Eigenes Vorhaben.

4. **`ServerDto::from_server` macht einen Schlüsselbund-Lesezugriff pro
   Server.** Auf einem gesperrten Linux-Keyring kann `list_servers` damit
   N Entsperr-Prompts auslösen. Vorbestehend, durch diese Spec nur sichtbar
   geworden. Die Laufzeitmessung (A0.4, s. Entscheidung 7) entlastet das
   nur teilweise: Sie deckt den *fehlenden*, nicht den *gesperrten*
   Anbieter ab — und ein Entsperr-Prompt kostet nicht Millisekunden,
   sondern eine Nutzerinteraktion.

5. **`sanitize_path_for_display` filtert nur `char::is_control()`.**
   U+2028 (Zeilentrenner) und U+202E (Richtungswechsel) kommen durch.
   Vorbestehend, von dieser Spec nicht berührt.

6. **Verbindungstest über einen Jump-Host zeigt weiterhin den rohen
   Bibliothekstext.** Die Secrets vorgelagerter Hops liest der Connector
   über den `TieredCredentialStore` und `core::ssh::auth::resolve_auth`;
   ein Fehler landet dort als `SshError::CredentialResolutionFailed` und
   im Formular als „✗ Netzwerkfehler: Passwort: No default store …".
   Der reguläre Verbindungsaufbau ist davon nicht betroffen (dort greift
   `SSH_CREDENTIAL_RESOLUTION_FAILED`, das im Frontend übersetzt wird) —
   nur der Verbindungstest. Das sauber zu schließen hieße, den Zustand
   durch `core` zu fädeln oder eine `TestConnectionResult`-Variante zu
   ergänzen; beides geht über den Umfang dieser Spec hinaus. Eigenes
   Vorhaben.

7. **`cleanup_abandoned_slots` loggt als einziger verschluckter `delete`
   nicht.** Er ist nach der X6-Korrektur zu Recht ein Aufräumpfad (Wechsel
   der Auth-Methode), aber wenn er scheitert, verschwindet ein verwaistes
   Secret spurlos — und `delete_auth_method_secrets` räumt später nur die
   Slots der *aktuellen* Methode ab, meldet den Altbestand also auch nicht
   in `secrets_left_behind`. Eigenes Vorhaben; die Abhilfe
   (`delete_all_possible_server_secrets` beim Löschen statt nur die
   aktuelle Methode) berührt Spec 0047.

8. **Die Rückstandsliste geht verloren, wenn der DB-Löschvorgang danach
   scheitert.** `servers.rs` bricht dann mit `?` ab; die Secrets sind ggf.
   schon weg, der Server bleibt. Vorbestehende Reihenfolge, durch A17 nicht
   schlechter geworden — aber die neue Meldung erreicht den Nutzer in genau
   diesem Fall nicht.

9. **Keine Steuerzeichen-/Bidi-Bereinigung für die Refs im
   Rückstandshinweis.** React escaped HTML, aber U+202E/U+2028 in einem aus
   der DB stammenden `CredentialRef` könnten den Hinweistext optisch
   verfälschen. Setzt Schreibzugriff auf die DB voraus; gleiche Klasse wie
   Punkt 5.

10. **Keine vollständige Testabdeckung für `ServerForm.tsx`.** Die Datei
   hatte bis zu dieser Spec gar keine Testdatei. Angelegt wurde eine, die
   genau die zwei Stellen abdeckt, die diese Spec ehrlicher macht (A14 und
   A17) — nicht mehr. Eine Rundum-Abdeckung des Formulars ist ein eigenes
   Vorhaben.
