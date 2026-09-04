# Spec: Session-Persistenz — Nachzieharbeiten aus dem Abgleich

Status: Entwurf
Modul: `crates/app-tauri` (Orchestrierung, Commands, MCP-Backend),
`persistence-sqlite`, `frontend/`
Abhängigkeiten: Session-Persistenz (0034), Chat-Verschlüsselung (0036),
Fencing/Eskalation (0039), MCP-Server (0028), Notiz-Vorschlag (0003/0010),
Prompt-Historie (0015)

> **Nummerierung**: nächste freie Nummer in deiner Reihe. Behebt die als
> "kleiner Fix" eingestuften Funde des Sitzungs-Abgleichs; das reiche
> Session-Modell (Kriterium 3, Ledger/Summary) ist ausdrücklich **nicht**
> Teil dieser Spec, sondern ein eigener späterer Architektur-Schritt
> (siehe technical-debt-backlog).

## 1. Ausgangslage

Der Sitzungs-Abgleich gegen Architektur-Brief §7 hat mehrere Funde ergeben,
die alle auf demselben Code-Pfad sitzen (`push_history` / `connect_session` /
Vorbereitung vor `AiProvider::send()`). Sie werden hier gebündelt, weil ein
gemeinsames Test-Gerüst sie zusammen effizienter absichert als einzeln. Der
schwerwiegendste Punkt ist eine **Verkettung**: Nutzer-Nachrichten landen
gar nicht in der (verschlüsselten) `chat_messages`-Tabelle, wodurch die
einzige persistierte Kopie des Nutzertexts unverschlüsselt in
`prompt_history` liegt — der Verschlüsselungszweck von 0036 wird auf der
Eingabeseite verfehlt.

## 2. Fund #1 — Nutzer-Nachrichten werden nicht persistiert

`send_chat_message` schreibt den Nutzertext direkt in den In-Memory-Kontext
und umgeht `push_history` (die einzige Funktion, die in `chat_messages`
schreibt). Spec 0034 §4 nennt Nutzertext ausdrücklich als eine der vier
fortlaufend zu persistierenden Nachrichtenarten.

**Fix**: `send_chat_message` schreibt die Nutzer-Nachricht über
`push_history` (also verschlüsselt, redigiert wo zutreffend), bevor der
KI-Aufruf startet. **Regressionstest, der bei `send_chat_message` einsteigt**
— nicht bei `run_chat_turn`/`push_history` —, da genau diese Test-Einstiegs-
lücke den Fund verdeckt hat.

## 3. Fund #2 — `prompt_history` wird verschlüsselt (Auflösung von 0036 §6)

Spec 0036 §6 hatte offengelassen, ob `prompt_history` mitverschlüsselt wird.
Durch Fund #1 ist die Entscheidung sicherheitsrelevant geworden: Solange (und
auch nachdem) #1 behoben ist, ist `prompt_history` freier Nutzertext, der
sensibel sein kann.

**Entscheidung/Fix**: `prompt_history.content` wird über denselben
`ContentCipher`-Mechanismus wie `chat_messages` verschlüsselt (Spec 0036,
Abschnitt 3/4 — dieselbe Cipher, derselbe Schlüssel, kein zweiter
Mechanismus). Migration `content` TEXT → BLOB analog zu 0036. Falls zum
Zeitpunkt der Umsetzung bereits Klartext-Zeilen existieren: einmaliges
idempotentes Migrations-Skript wie in 0036 §5.

## 4. Fund #6 — MCP-Ergebnisse landen in wiederaufnehmbarer Menschen-Historie

`connect_session(..., resume: None)` für ein MCP-ausgelöstes, noch nicht
offenes Tab legt bedingungslos eine `chat_sessions`-Zeile an, und
`push_history` schreibt MCP-Ergebnisse hinein. Spec 0034 §10 schließt genau
das aus. Der Kommentar bei `orchestration.rs:100-102` ("läuft nie für MCP")
ist faktisch falsch. Ist bereits ein Menschen-Tab für den Server offen,
mischen sich MCP-Ergebnisse in dessen persistierte, wiederaufnehmbare
Historie.

**Fix**:
- MCP-ausgelöste Aktionen erzeugen **keine** `chat_sessions`-Zeile und
  schreiben **nichts** über `push_history` in die persistierte Historie —
  konsistent mit Spec 0034 §10 und der bereits in Spec 0028/0037
  etablierten Trennung.
- Entscheidung (hiermit getroffen): Eine MCP-Aktion, die in einen bereits
  offenen Menschen-Tab hineinfällt, wird **nicht** in dessen persistierte
  Historie geschrieben. Sie erscheint im Live-UI (Bestätigungsdialog,
  Ergebnis), aber der wiederaufnehmbare Cache bleibt frei von
  MCP-Ursprungs-Inhalten.
- Den falschen Kommentar korrigieren.
- Regressionstest: MCP-Aktion erzeugt keine `chat_sessions`-Zeile und keinen
  `chat_messages`-Eintrag, auch wenn ein Menschen-Tab für denselben Server
  offen ist.

Hinweis: Der `untrusted_content_ingested`-Flag greift laut Abgleich korrekt
(keine Filter-Engine-Umgehung) — das bleibt so, unabhängig von diesem Fix.

## 5. Fund #2 (Invariante) — Redaction läuft beim Senden erneut

Aktuell ist Redaction ein einmaliges Write-Time-Gate; vor `AiProvider::send()`
läuft nur die Budget-Truncation. Szenario: Eine Kommando-Ausgabe mit einem
Secret wird persistiert, *bevor* das passende Sudo-Passwort hinterlegt ist
(Default-Redactor kennt das Muster noch nicht); nach späterem Hinterlegen
sendet ein Resume die alte, weiterhin unredigierte Historie wörtlich erneut.

**Fix**: Vor jedem `AiProvider::send()` läuft ein Redaction-Durchlauf über
`request_context.history`. **Nur additiv** — dieser Durchlauf darf nur
zusätzlich redigieren, **nie** bereits vorhandene Redaction entfernen oder
Inhalt verändern, der bereits gesehen wurde. Das ist die Auflösung der
Spannung zwischen Brief-Kriterium 1 ("Resume sendet exakt das, was der
Provider schon sah") und Kriterium 2 ("Redaction läuft erneut"): Neue
Muster werden angewendet, aber es kcommt nie *mehr* Inhalt heraus als beim
ersten Mal. `OutputRedactor` muss dafür sauber auf `MessageContent::Text`
anwendbar sein (arbeitet aktuell auf `CommandOutput`). Eigener Test für die
nur-additive Richtung (nachträglich hinzugefügtes Muster redigiert alte
Zeile beim Resend; kein zuvor redigierter Inhalt wird wieder sichtbar).

## 6. Fund #4 — "In Notiz übernehmen"-UI-Aktion

Brief-Kriterium 4 verlangt eine UI-Aktion "in Notiz übernehmen"; sie
existiert nicht. Die konzeptionelle Trennung Cache ≠ Notizen ≠ Audit-Log
hält, nur die Affordanz fehlt.

**Fix**: Eine Aktion (z. B. Button/Kontextmenü an einer Chat-/Ergebnis-Zeile),
die den bestehenden `ProposeNoteUpdate`-Flow (Spec 0003 Abschnitt 5.2) mit
dem Inhalt der Zeile vorbefüllt. Kein neuer Persistenz-Mechanismus — nutzt
den bestehenden Notiz-Vorschlag-Pfad, inklusive dessen Bestätigungsdialog.

## 7. Trivial-Fixes (klein, unabhängig, hier mit erledigt)

1. **`.expect()` auf den Verschlüsselungsschlüssel** (`lib.rs:67-68`) panict
   die **gesamte** App beim Start, wenn der OS-Schlüsselbund gesperrt/
   verweigert ist. Spec 0036 §4 wollte Lazy-Generierung "beim ersten
   Schreibzugriff". Fix: Fehler beim Schlüsselzugriff darf nur die
   Chat-Persistenz betreffen (klare Fehlermeldung, Chat-Cache deaktiviert),
   nicht die ganze App abbrechen — die DB-Analogie im Kommentar trägt nicht
   (ohne DB geht nichts, ohne Chat-Cache läuft fast alles weiter).
2. **Hängender Transport bei fehlgeschlagenem Resume** (`commands.rs:561-563`):
   Schlägt `load_session`/`mark_resumed` fehl, nachdem der SSH-Transport
   bereits verbunden ist, wird via `?` zurückgekehrt, ohne den Transport
   explizit zu trennen (nur Drop). Fix: sauberer Disconnect im Fehlerpfad.
3. **Löschen einer aktiven Session** (`delete_chat_session`) prüft nicht, ob
   die Session gerade aktiv ist — die Live-Session schreibt danach in einen
   Foreign-Key-Void, nur als `warn!` sichtbar. Fix: aktive Session vor dem
   Löschen erkennen und entweder verhindern (mit klarer Meldung) oder sauber
   beenden.

## 8. Fehlende ADRs (mit erledigen)

- ADR für die **hartcodierte Truncation-Budget-Konstante** (Spec 0034 §9
  fragte nach "konfigurierbar"; ein `const` wurde ausgeliefert — entweder
  konfigurierbar machen ODER die bewusste Vereinfachung als ADR
  dokumentieren; Empfehlung: ADR, das Budget ist kein Nutzer-Bedienknopf).
- ADR für die **Chat-Persistenz-Ausnahme des lokalen Pseudo-Servers**
  (korrekt wegen fehlender `servers`-Zeile für den FK, aber undokumentiert).
- Der `content_type = 'document'`-CHECK-Wert im Schema ist toter Code
  (GenerateDocument wird als `Text`/`Assistant` gespeichert). Entweder
  tatsächlich als eigener Typ produzieren oder aus dem Schema entfernen —
  kleine Aufräumentscheidung, im ADR mitvermerken.

## 9. Nicht Teil dieser Spec

- Das reiche Session-Modell (Ledger, Summary, Kompaktierung, §4.4/§7
  Kriterium 3) — eigener späterer Architektur-Schritt (Backlog).
- Der Auto-Titel-/Notiz-Vorschlag-Aufruf-Merge (gehört zum Summary-Modell,
  Backlog).

## 10. Sicherheits-Invarianten

- Nutzer-Nachrichten landen in der **verschlüsselten** `chat_messages`; die
  persistierte Kopie in `prompt_history` ist **ebenfalls verschlüsselt**.
- MCP-Ursprungs-Inhalte landen **nie** in einer persistierten, wieder-
  aufnehmbaren `chat_sessions`-Historie.
- Der Re-Redaction-Durchlauf beim Senden ist **nur additiv** — kein zuvor
  redigierter oder nie gesendeter Inhalt wird durch ihn sichtbar.

## 11. Testbarkeit

- Regressionstest ab `send_chat_message`, der belegt, dass Nutzertext in
  `chat_messages` persistiert wird.
- Test, dass `prompt_history.content` verschlüsselt auf Disk liegt (Direct-
  SQL-Bypass wie in 0036).
- Test, dass eine MCP-Aktion (auch bei offenem Menschen-Tab) keine
  `chat_sessions`-/`chat_messages`-Zeile erzeugt.
- Test der nur-additiven Re-Redaction (nachträgliches Muster greift beim
  Resend, nichts zuvor Redigiertes wird sichtbar).
- Trivial-Fixes: gesperrter Keychain lässt die App starten (Chat-Cache
  deaktiviert, kein Panic); fehlgeschlagener Resume trennt den Transport
  sauber; Löschen einer aktiven Session ohne FK-Void.
