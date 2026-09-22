# XXXX-keychain-unavailable-reporting-decisions

> **Nummer noch offen.** Die Aufgabenstellung gab keine ADR-Nummer vor;
> `XXXX` ist ein Platzhalter und wird beim Zusammenführen vergeben (s.
> `docs/adr/README.md`).

## Status

Proposed — gehört zu Spec 0071.

## Kontext

Spec 0071 lässt an mehreren Stellen offen, *wie* etwas umgesetzt wird, und
eine Vorgabe (Teil 0) wurde während der Umsetzung geändert. Dieses ADR hält
die Stellen fest, an denen bei der Umsetzung eine Wahl getroffen wurde, die
die Spec nicht wörtlich vorgibt — damit ein späterer Leser nicht
rekonstruieren muss, ob das Absicht war.

## Entscheidungen

### 1. Die Paketnamen sind nicht gemessen (`ANNAHME A-1`)

Spec 0071 §3 A0.3 verlangt die Paketnamen „nachgewiesen durch `apt install`
und einen anschließend erfolgreichen Provider-Anlegeversuch, nicht aus dem
Gedächtnis". Die dafür nötige Linux-Umgebung stand bei der Umsetzung nicht
zur Verfügung.

**Entscheidung (2026-09-22, als Klarstellung in §9 der Spec festgehalten):**
Trotzdem bauen. Die Paketnamen stammen aus §1.1/A5/A6 der Spec selbst
(`gnome-keyring`, `kwalletd6`, `dbus-user-session`, KeePassXC).

**Konsequenz:** Solange die Annahme nicht durch die manuellen Tests M1–M4
bestätigt ist, kann Smart SSH auf einer echten Debian-Minimal-Installation
den falschen Paketnamen nennen. Die Bestätigung ist Vorbedingung für das
Zusammenführen, kein optionaler Nachtrag.

**Fundstellen** (alle fünf, weil ein `grep` allein sie nicht findet — die
Locale-Dateien können keinen Kommentar tragen):

1. `startup_error_messages.rs`, `NoSecretServiceProvider`-Arme (DE + EN),
2. ebenda, `NoSessionBus`-Arme (DE + EN),
3. `locales/de/common.json` → `diagnostics.keychainUnavailable.*`,
4. `locales/en/common.json` → dieselben Schlüssel,
5. `changelog.d/0071-linux-secret-service-meldung.md`.

Zusätzlicher Zweifel aus dem Review, der bei M2/M3 ausdrücklich zu prüfen
ist: `gnome-keyring` allein bringt auf einer Minimal-Installation
vermutlich **keinen** laufenden Secret Service — es fehlen wahrscheinlich
`dbus-user-session` und `libpam-gnome-keyring` (Entsperren beim Login).
Ebenso ist `kwalletd6` ohne `kwallet-pam` fraglich. Der Text sagt
„einrichten und danach neu anmelden", was das teilweise abfedert; die
Paketliste ist aber wahrscheinlich zu kurz.

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

**Konsequenz:** Sperrt sich der Schlüsselbund erst *nach* dem Start
(Bildschirmsperre nach Zeitablauf), bleibt der Zustand weiterhin stale.
Siehe offener Punkt 1 unten.

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

**Konsequenz:** X1 ist per Konstruktion erfüllt und braucht keine
Bereinigung analog `sanitize_path_for_display`. Der Preis: Der Wert steht
auch nicht im Log.

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

2. **`clear_sudo_password` und `delete_server` melden Erfolg, auch wenn
   das Secret nicht gelöscht werden konnte.** `clear_sudo_password`
   schluckt den Fehler (`let _ = delete(...)`), das Kommando liefert
   unbedingt `Ok(())`, und die Oberfläche setzt daraufhin „kein
   Sudo-Passwort hinterlegt". Bei nicht verfügbarem Schlüsselbund bleibt
   das Secret dann im Schlüsselbund, während die App Erfolg meldet.
   Dasselbe gilt für `delete_auth_method_secrets` beim Löschen eines
   Servers: Die DB-Zeile verschwindet, die Secrets bleiben als verwaiste
   Einträge zurück — ohne die Logzeile, die
   `delete_all_possible_server_secrets` für den Rollback-Pfad bereits hat.
   Spec 0071 §6.3 X6 führt genau diese Aufrufe ausdrücklich als
   „Aufräumpfade" auf, die verschluckt bleiben sollen. Das trifft auf ein
   bewusstes „Entfernen"-Kommando nicht zu — die Spec-Annahme ist hier
   falsch. Die Korrektur ändert Verhalten und berührt eine
   Sicherheits-Invariante (kein falsches Erfolgssignal), gehört also
   entschieden und nicht vom Umsetzenden angenommen. Abgemildert wurde nur
   der Teil, der in dieser Spec neu entstand: Die Löschvorschau behauptet
   keine Entfernung mehr.

3. **Ein nicht lesbares Sudo-Passwort schaltet still eine Redaktionsschicht
   ab.** `commands.rs` macht an zwei Stellen aus jedem `CredentialError`
   ein `None`. Der `sudo -S`-Pfad läuft dann ohne Passwort weiter (fail-safe),
   aber der Redactor bekommt das Sudo-Passwort-Muster nicht — obwohl der
   Kommentar daneben begründet, warum dieses Muster sicherheitsrelevant
   ist. Praktisch begrenzt (ohne lesbaren Schlüsselbund gelangt das
   Passwort auch nicht neu auf den Server), aber es ist eine stille
   Abschwächung und gehört mindestens geloggt. Außerhalb des Umfangs
   dieser Spec.

4. **`risk_second_opinion.rs` verschluckt den Credential-Fehler mit
   `.ok()?`.** Fail-safe im Sinne von ADR 0024 (die Zweitmeinung kann nur
   eskalieren, ihr Ausfall senkt keine Einstufung), aber unsichtbar: Ohne
   Schlüsselbund ist sie für die ganze Sitzung stumm aus. Eigenes Vorhaben.

5. **`ServerDto::from_server` macht einen Schlüsselbund-Lesezugriff pro
   Server.** Auf einem gesperrten Linux-Keyring kann `list_servers` damit
   N Entsperr-Prompts auslösen. Vorbestehend, durch diese Spec nur sichtbar
   geworden; passt zu der nie gemessenen Laufzeitfrage A0.4.

6. **`sanitize_path_for_display` filtert nur `char::is_control()`.**
   U+2028 (Zeilentrenner) und U+202E (Richtungswechsel) kommen durch.
   Vorbestehend, von dieser Spec nicht berührt.

7. **Kein Komponententest für den neutralen Sudo-Zustand im
   Server-Formular.** `ServerForm.tsx` hat bislang überhaupt keine
   Testdatei; die DTO-Seite (A14/X4) ist im Backend geprüft. Eine
   Testdatei nur für dieses eine Feld anzulegen wäre unverhältnismäßig —
   der Punkt gehört in die manuellen Tests.
