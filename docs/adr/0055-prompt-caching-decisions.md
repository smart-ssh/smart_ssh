# 0055 — Prompt-Caching (Spec 0064): Breakpoint-Platzierung, Banner-Umzug,
Kompaktierungs-Zusammenspiel

## Status

Angenommen

## Kontext

Spec 0064 verlangt Anthropic-`cache_control`-Breakpoints auf dem stabilen
Präfix eines Requests (System-Prompt + Werkzeuge + Server-Notiz), damit
gecachte Input-Tokens nicht gegen das ITPM-Rate-Limit zählen. Die Spec
verlangt außerdem, vor der Umstellung den Ist-Aufbau des Requests und alle
gefundenen Cache-Killer zu berichten (Teil 0) und das Zusammenspiel mit der
Kompaktierung (Spec 0057) explizit zu analysieren (Teil 6). Dieser ADR hält
die dabei getroffenen Entscheidungen fest.

## Entscheidungen

### 1. Zwei Breakpoints: Ende der Werkzeuge, Ende des System-Blocks

`crates/ai-providers/src/anthropic.rs::build_request_body` setzt
`cache_control: {"type": "ephemeral"}` an zwei Stellen:

- Auf dem **letzten** Element des `tools`-Arrays.
- Auf dem (einzigen) Content-Block des `system`-Felds.

Anthropics interne Prompt-Reihenfolge ist unabhängig von der JSON-
Feldreihenfolge im Request immer "Werkzeuge → System → Nachrichten". Ein
Breakpoint auf dem letzten Werkzeug cacht damit **nur** die Werkzeuge — ein
Eintrag, der über ALLE Sitzungen/Server hinweg identisch ist (der
Werkzeug-Satz kommt unverändert aus `default_action_schemas()`, s.
Abschnitt 4) und sich deshalb sitzungs- und serverübergreifend
wiederverwenden lässt. Der zweite Breakpoint auf dem System-Block cacht
zusätzlich die server-spezifische Notiz — dieser Eintrag ist an die
konkrete Server-Identität (genauer: an den exakten Byte-Inhalt von
`context.system_context`) gebunden, aber der GRÖSSERE Gewinn laut Spec
0064s eigener Einschätzung ("der Haupt-Cache-Gewinn").

Beide zusammen: 2 von maximal 4 erlaubten Breakpoints (verifiziert gegen
die aktuelle Anthropic-Doku, `anthropic-version: 2023-06-01`, kein
Beta-Header nötig — Prompt-Caching ist inzwischen GA).

**Kein eigener Breakpoint auf `messages`.** Spec 0064 fokussiert
ausdrücklich auf System-Prompt + Werkzeuge + Notiz als "den Haupt-Gewinn";
den wachsenden Gesprächsverlauf zusätzlich zu cachen wäre ein separater,
eigenständiger Schritt (mit eigener Abwägung: der 5-Minuten-TTL des
Ephemeral-Caches greift bei einer wachsenden `messages`-Historie anders
als beim stabilen System-Block) — hier bewusst nicht Teil dieses Schritts.

### 2. `system` unbedingt als Array mit `cache_control`, keine
Mindestlängen-Prüfung

`system` wechselt von einem reinen String zu einem Ein-Block-Array
(Anthropic erlaubt `cache_control` nur auf Content-Blöcken). Der
Breakpoint wird **immer** gesetzt, unabhängig von der tatsächlichen Länge
des System-Prompts. Anthropics Dokumentation bestätigt: ein zu kurzer
Block (unterhalb einer modellabhängigen Mindestlänge im niedrigen drei-
bis vierstelligen Token-Bereich) wird einfach ohne Caching verarbeitet,
kein Fehler — die genaue Zahl hängt vom konkreten Modell ab und ist für
diese Entscheidung irrelevant. Eine eigene Mindestlängen-Prüfung vor dem
Setzen von `cache_control` hätte denselben Effekt nur mit zusätzlichem
Code erreicht — bewusst weggelassen.

### 3. Der `uname`-Banner zieht aus dem System-Prompt in eine eigene,
gefencte Verlaufs-Nachricht

Vorher: `SystemContextParts::assemble_with_notes` hängte den sanitisierten
`uname -a`-Output als `"\n\n## Remote-System\n{os}"` roh ans Ende des
System-Prompts — der EINZIGE der vier in Spec 0039 Abschnitt 1 genannten
Untrusted-Content-Fälle, der nie durch `fence_untrusted` lief (offener
Backlog-Punkt, mit diesem Schritt miterledigt).

Für Prompt-Caching ist das strukturell riskant: `system_context` wird bei
JEDER Nutzer-Nachricht neu zusammengesetzt (Spec 0039 Abschnitt 5) und ist
genau der Text, auf den der `cache_control`-Breakpoint zeigt — jedes Byte
darin ist Teil des zu cachenden Präfix. Der Banner selbst ändert sich zwar
in der Praxis kaum (einmalig bei `connect()` per `uname -a` gelesen, für
die Sitzungsdauer stabil), aber ihn dennoch strukturell aus diesem Präfix
herauszuhalten vermeidet jede stille Abhängigkeit von dieser Annahme.

Lösung: neue `UntrustedKind::RemoteOsInfo`-Variante (Tag `<remote_system>`,
`crates/core/src/ai/fencing.rs`), der Banner wird einmalig bei
`connect_session` als gefencte `ChatMessage` VOR die (Resume- oder
frische) Historie gestellt — nicht DIREKT persistiert (dasselbe "bei
jedem `connect()` frisch"-Muster wie der System-Prompt selbst; s. aber
den Nachtrag unten zur rollierenden Zusammenfassung, wo er DOCH indirekt
landen kann), sodass auch eine wiederaufgenommene Sitzung (deren
persistierte Historie den Banner strukturell nie enthielt, da er früher
Teil des Systems-Prompts war) ihn bekommt.

**Nebenfund beim Umbau, direkt mitbehoben:** `session.rs::
history_contains_untrusted_content` pflegte eine eigene, hartcodierte
`FENCE_OPEN_TAGS`-Liste, unabhängig von `fencing.rs`s `UntrustedKind`-Enum
— ohne Anpassung hätte eine Sitzung, deren einziger Untrusted-Inhalt der
neue Banner ist, `untrusted_content_ingested` fälschlich nie gesetzt
(Spec-0039-Regression). Auf `ssh_manager_core::ai::fence_markers()`
umgestellt, damit diese Liste nicht mehr separat gepflegt werden muss.

**Verhaltensänderung, spec-reviewer-Fund (Follow-up-Review), hier
nachträglich dokumentiert statt nur implizit in Kauf genommen:** weil der
Banner jetzt gefencter Untrusted-Inhalt IN DER HISTORIE ist (vorher stand
er ungefenct und ohne jede Kennzeichnung im System-Prompt), startet jede
Sitzung mit einem vom Server gelesenen `uname`-Ergebnis jetzt mit
`untrusted_content_ingested = true` ab der ERSTEN Runde — bei
`PostIngestPolicy::Strict` wird dadurch jedes `AutoExec` sofort zu
`Confirm` eskaliert, bei `Balanced` jede modifizierende Aktion; Allow-
Regeln greifen für solche Server praktisch nie mehr ohne Nachfrage.
Die Richtung ist sicher (Spec 0039: nur Eskalation, nie Abschwächung) und
inhaltlich korrekt (Serverinhalt IST eingelesen worden — vorher lief
exakt derselbe Inhalt sogar ungefenct und flaglos in den System-Prompt,
das hier ist eine echte Verbesserung, kein Nebenschaden). Praktisch
abgefedert: `sanitize_uname_output` lässt kein `/` zu, ein typisches
Linux-`uname -a` ("…GNU/Linux…") oder macOS-Ergebnis fällt also meist
schon durch die Zeichen-Whitelist (→ `None`, kein Banner, keine
Flag-Änderung) — der Effekt tritt vor allem bei knapperen `uname`-
Ausgaben ohne Slash auf. Bewusst nicht als Bug behandelt, nur hier
dokumentiert, weil es weder in Spec 0064 noch in der ursprünglichen
Fassung dieses ADR auftauchte.

### 4. Cache-Killer-Bestand (Teil 0, wie berichtet)

Geprüft und **nicht** gefunden: kein Datum/Uhrzeit-Literal irgendwo im
System-Prompt-Aufbau; kein sich mitten in einer Sitzung ändernder
Werkzeug-Satz (`available_actions` ist an jeder Produktions-Konstruktions-
stelle `default_action_schemas()`, eine feste, unparametrisierte Funktion);
kein Modellwechsel innerhalb einer Aufruf-Kette (Modell ist pro
`AnthropicProvider`-Instanz fix, für die Sitzungsdauer unverändert).
Gefunden und behoben: der `uname`-Banner (Abschnitt 3 oben) — der einzige
tatsächliche Cache-Killer-Kandidat.

### 5. Zusammenspiel mit der Kompaktierung (Spec 0057, Teil 6)

Die Kompaktierungs-Leiter (`compact_for_send`) ist bereits — ohne dass
dieser Schritt sie dafür umbauen musste — cache-freundlich geordnet:

1. `compact_rounds_with_summary` (Runden falten) ändert nur `context.
   history`, nie `context.system_context`.
2. `compact_oversized_outputs_for_budget` (Ausgaben-Cap) ändert ebenfalls
   nur `context.history`.
3. `compact_notes_for_budget` (Notiz-Kürzung) — die EINZIGE der drei
   Stufen, die `context.system_context` (und damit den gecachten
   System-Block) verändert — läuft als LETZTER Ausweg, nur wenn die ersten
   beiden Stufen nicht reichten.

Für den Cache bedeutet das: Runden-Kompaktierung und Ausgaben-Cap sind
strukturell unkritisch (sie treffen nie den gecachten Präfix). Nur wenn
Stufe 3 tatsächlich greift, gibt es einen echten Cache-Miss für den
System-Block dieser einen Anfrage (neuer `cache_creation`-Eintrag für die
gekürzte Fassung). Das ist ein realer, aber eng begrenzter Konflikt:

- Die Kürzung ist PRO SEND ephemer (`compact_notes_for_budget` mutiert nur
  die für diesen einen Aufruf gebaute Kopie, nie `session.system_context_
  parts`/`session.context.system_context`) — die nächste Anfrage, sobald
  sie wieder unter das Budget passt, verwendet wieder den vollständigen,
  ORIGINAL-System-Block und kann (falls dessen Cache-Eintrag innerhalb der
  5-Minuten-TTL noch existiert) direkt wieder treffen.
- Bleibt der Kontext über mehrere aufeinanderfolgende Runden hinweg knapp
  über dem Budget, kürzt `compact_notes_for_budget` bei gleichbleibendem
  `budget_tokens`/gleichbleibender Notiz deterministisch auf dieselbe
  Byte-Folge — nach dem ersten Miss (neue `cache_creation`) treffen
  nachfolgende Runden also wieder, nur gegen den gekürzten statt den
  vollständigen Eintrag.
- Wird die Notiz-Kürzung dadurch teurer, als sie spart? Nein in der Praxis
  relevant: Stufe 3 ist der letzte Ausweg einer ohnehin schon seltenen
  Kompaktierung (nur wenn Runden-Kürzung + Ausgaben-Cap zusammen nicht
  reichen) — die zusätzlichen Kosten sind ein einzelner
  `cache_creation`-Request an der Übergangsstelle, kein wiederholter
  Verlust.

**Claude Codes "gecachten Aufruf forken statt neu bauen"-Muster** ist hier
nicht direkt anwendbar: das setzt voraus, dass sich der stabile Präfix
selbst NIE ändert und nur angehängt wird (reines Forken). Bei uns kann
genau der stabile Präfix (die Notiz) im Extremfall selbst kleiner werden
müssen — ein Fork-Mechanismus würde daran nichts ändern, das Kürzen selbst
ist unvermeidlich, sobald das Budget das verlangt. Die bereits bestehende
Prioritätsreihenfolge der Leiter (Notiz-Kürzung zuletzt, nicht zuerst)
erreicht in der Praxis denselben Effekt wie das Claude-Code-Muster
anstrebt: den cache-brechenden Schritt so selten wie möglich zu nehmen.
Keine Änderung an der Ladder-Priorität nötig oder vorgenommen.

### 6. Sichtbarkeit nur für Anthropic (Teil 5)

`log_cache_usage` (neu in `crates/ai-providers/src/request_logging.rs`)
wird nur vom Anthropic-Provider aufgerufen (`message_start`-SSE-Event,
Feld `message.usage`). OpenAI-kompatible Provider bekommen laut Spec 0064
Teil 4 explizit kein `cache_control` (OpenAI cacht automatisch) — ein
eigenes Usage-Logging für `prompt_tokens_details.cached_tokens` (OpenAIs
Äquivalent) wäre ein sinnvoller, aber eigenständiger Folge-Schritt, den
diese Spec nicht verlangt (Teil 5 spricht wörtlich von "jeder
Anthropic-Antwort").

## Konsequenzen

- `SystemContextParts` verliert das `remote_os_info`-Feld ersatzlos —
  jeder bisherige Aufrufer, der es brauchte, ist auf die neue,
  banner-tragende Verlaufs-Nachricht umgestellt.
- Eine bereits laufende, alte Sitzung (vor diesem Schritt verbunden) sieht
  den Banner nach einem Update erst nach einem Reconnect wieder — ihr
  `system_context` in-memory enthält noch die alte, im-System-Prompt-
  eingebettete Fassung, bis neu verbunden wird. Kein Datenverlust (der
  Banner war nie sicherheitskritisch für sich), nur eine Übergangs-
  Inkonsistenz für die Dauer eines laufenden App-Prozesses nach einem
  Update — nicht behoben, da ein Hot-Reload des System-Prompts einer
  laufenden Sitzung außerhalb des Scopes dieses Schritts liegt.
