# ADR 0088 — Entscheidungen bei der Umsetzung von Spec 0096 (Geheimnisse in der Datenbank)

Status: angenommen · Spec: `docs/specs/0096-db-secrets-at-rest.md` · Backlog: BL-0295

Spec 0096 verlangt, dass ein Geheimnis aus einem Kommando nicht im Klartext
in einer Datei des Datenbank-Verzeichnisses landet. Sie lässt mehrere Punkte
offen, die bei der Umsetzung entschieden werden mussten. Diese Datei hält
fest, was entschieden wurde und warum.

## 1. Schwärzung des Notizvorschlags an den Einstiegen, nicht im gemeinsamen Pfad

A2 verlangt die Schwärzung für die drei Wege, auf denen eine KI einen
Notizvorschlag erzeugt (Chat-Runde, Verbindungsende, MCP), **nicht** aber für
„In Notiz übernehmen", wo der Nutzer selbst eine Chatzeile auswählt
(Entscheidung E3 der Spec).

Der naheliegende Ort wäre `handle_action_proposed` gewesen — eine Stelle
statt drei. Das geht nicht: „In Notiz übernehmen"
(`propose_note_from_chat_content`) ruft dieselbe Funktion mit derselben
`ActionOrigin::Internal` auf wie die Chat-KI. An der Herkunft sind die beiden
dort nicht mehr zu unterscheiden, und ein zusätzliches Unterscheidungsmerkmal
nur für diesen Zweck einzuführen hieße, eine Sicherheitsentscheidung von
einem neuen, leicht zu vergessenden Parameter abhängig zu machen.

Entschieden: ein gemeinsamer Helfer `redact_note_proposal`, aufgerufen an den
drei KI-Einstiegen. `propose_note_from_chat_content` ruft ihn bewusst nicht
auf. Der Gegenfall-Test T12 ist der Wächter dafür — wandert die Schwärzung
später doch in den gemeinsamen Pfad, wird T12 rot.

**Folge, die bei der Handabnahme zu kennen ist:** „In Notiz übernehmen" steht
in der Oberfläche auch an Kommando-Ergebnis- und Datei-Inhalts-Karten, deren
Inhalt roh angezeigt wird. Ein Klick dort schreibt diesen Rohinhalt
unverändert in die Notiz. Das ist durch E3 gedeckt (der Nutzer wählt aus),
trifft aber die Formulierung „Notizen, die der Nutzer selbst schreibt" aus
§3 der Spec nicht ganz. Ein Klartext-Treffer in `servers.notes` nach einem
solchen Klick ist erwartetes Verhalten, kein Fehler.

## 2. Ein vollständig geschwärzter Vorschlag wird gemeldet, nicht still verworfen

Die Spec sagt nur: „Ist sie leer, entsteht kein Vorschlag." Wie der Nutzer
davon erfährt, lässt sie offen.

Entschieden: Im Chat-Weg und im MCP-Weg wird zusätzlich ein `chat-error` mit
einem festen Text emittiert (der Text selbst trägt keinen Inhalt der KI). Eine
Aktionskarte, die einfach ausbleibt, sähe für den Nutzer — und für einen
MCP-Client, der sonst ein irreführendes „vom Nutzer abgelehnt" bekäme — wie
ein Fehler der App aus.

**Beim Verbindungsende bleibt es bei einem Log-Eintrag.** Dort gibt es keinen
sichtbaren Chat mehr, an den eine Meldung gehen könnte; Spec 0010, Abschnitt 2,
Punkt 4 legt für diesen Weg ausdrücklich „kommentarlos beenden, kein
`chat-error`" fest. Die Asymmetrie zur Chat-Runde ist gewollt und folgt der
älteren Festlegung, statt sie zu unterlaufen.

## 3. Ein verworfener Vorschlag zählt als Ablehnung für Folgeaktionen

Der Frühausstieg für einen vollständig geschwärzten Vorschlag umgeht
`handle_action_proposed` — und damit auch die Stelle, die sonst
`earlier_rejection` setzt (Spec 0068, Teil 4). Ohne Gegenmaßnahme könnte auf
einen verworfenen Vorschlag in **derselben** KI-Antwort eine per Allow-Regel
freigegebene Aktion ohne Rückfrage folgen.

Entschieden: Der Frühausstieg setzt die Flagge selbst. Dass die KI gerade
einen Vorschlag geliefert hat, der vollständig aus erkannten Zugangsdaten
bestand, ist mindestens so verdächtig wie eine Ablehnung durch den Nutzer,
die genau diese Eskalation auslöst. Die Änderung wirkt ausschließlich in
Eskalationsrichtung: die Flagge kann eine Entscheidung nur von `AutoExec`
nach `Confirm` bewegen, nie umgekehrt.

## 4. Abweichung von der Spec: T6 prüft andere Längen als dort genannt

Spec 0096 §7 beschreibt T6 mit „55 Zeichen Text und danach
`password=Geheim-0096`". **Gemessen kann der Test in dieser Form nicht
scheitern:** `password=` beginnt dann an Position 55, das Kürzen auf 60
Zeichen schneidet bereits im Schlüsselwort, und der Geheimniswert erreicht
den Titel gar nicht — auch dann nicht, wenn man Schwärzen und Kürzen
vertauscht, also genau den Fehler baut, den T6 finden soll.

Entschieden: T6 prüft den Spec-Fall weiterhin als Grenzfall, zusätzlich aber
45 Zeichen (der Wert läuft dann über die Grenze) und einen Fall mit
**nachlaufendem Anker** (`https://u:<wert>@host`). Nur der letzte trägt den
Gegenbeweis: die URL-Regel braucht das `@host` hinter dem Wert; liegt es
jenseits der Kürzungsgrenze, erkennt der Redactor nach einem vorgezogenen
Kürzen gar nichts mehr und der Wert stünde vollständig im Titel. Das wurde
durch einen Lauf mit vertauschter Reihenfolge belegt.

Die Abweichung verschärft den Test und ändert nichts an der Anforderung A1.
Sie steht als Klarstellung in §9 der Spec.

## 5. Zusätzlicher Nachweis bei offener Datenbank

A3 verlangt, nach `pool.close()` jede Datei des Verzeichnisses zu
durchsuchen. **Gemessen hinterlässt ein sauber geschlossener Pool keine
`-wal`-Datei mehr** — T3 durchsucht deshalb faktisch nur die Hauptdatei, und
die von der Spec selbst genannte Angriffsrichtung „Rohdatei-Suche, die nur
die Hauptdatei liest (WAL)" bliebe unbelegt.

Entschieden: ein dritter Test sucht **vor** dem Schließen. Dort existieren
`-wal` und `-shm` und werden mitdurchsucht. Er deckt zugleich den
realistischeren Zustand ab — die Datenbank einer laufenden App. Ein Lauf mit
durchreichendem Cipher findet das Geheimnis in genau dieser `-wal`-Datei; der
Fall ist also nicht leer.

## 6. Grenzen, die dieser Schritt bewusst nicht schließt

- **Nur UTF-8-Bytes.** Der Rohdatei-Nachweis sucht die UTF-8-Darstellung des
  Geheimnisses. Läge dasselbe Geheimnis in UTF-16, Base64 oder komprimiert in
  einer Datei, fände die Suche es nicht. Für die geprüften Schreibpfade ist
  UTF-8 die einzige entstehende Kodierung; für neue Schreibpfade gilt das
  nicht automatisch.
- **Der Nachweis trägt so weit wie der Redactor.** §3 der Spec formuliert das
  Ziel absolut („enthält keine Datei das Geheimnis im Klartext"). Der
  Mechanismus erkennt aber nur, was seine Muster erfassen. Insbesondere kennt
  das Muster-Set keine deutschen Schlüsselwörter (`Passwort:`, `Kennwort=`),
  während der Titel-Prompt ausdrücklich einen deutschen Titel verlangt — ein
  eigenes Thema, das Spec 0096 §3 als Nicht-Ziel ausschließt (neue
  Redactor-Muster).
- **Nebenwirkung der von A1 geforderten Reihenfolge:** Weil das Schwärzen den
  Text verkürzt, kann Inhalt, der vorher jenseits der 60-Zeichen-Grenze lag
  und weggekürzt wurde, jetzt in den gespeicherten Titel rutschen. Die
  Kürzung war nebenbei eine Längenbremse, die nun später greift. In Summe ist
  die geforderte Reihenfolge deutlich sicherer; der Restfall gehört zum
  Redactor-Thema oben.
- **Der Gegenbeweis T4 prüft grob.** Er fordert nur, dass die Suche das
  Geheimnis überhaupt findet. Schriebe künftig eine der fünf Senken still
  nichts mehr, bliebe T4 grün, und T3 wäre für diese Senke eine leere
  Zusicherung. Eine schärfere Form wäre ein eigener Marker je Senke.
