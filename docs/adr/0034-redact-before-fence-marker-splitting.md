# 0034-redact-before-fence-marker-splitting

## Status
Vorgeschlagen

## Kontext

Ein unabhängiger `spec-reviewer`-Durchlauf über Spec 0040 fand: der
additive Re-Redaction-Durchlauf aus Spec 0040, Abschnitt 5
(`orchestration::reapply_redaction_for_send`, läuft unmittelbar vor jedem
`AiProvider::send()`-Aufruf) konnte über bereits gefencten Inhalt laufen.
`execute_read_remote_file` fenct `RemoteFile`-Inhalt (Spec 0039,
`fence_untrusted`) in `<remote_file>...</remote_file>`, **bevor** der
gefencte String in `session.context`/die DB geschrieben wird — genau
dieser bereits gefencte String ist die persistierte Repräsentation, die
später erneut redigiert wird. Ein gieriges Redaction-Fallback-Muster
(z. B. das Private-Key-Rückfallmuster für einen abgeschnittenen Key-Block
ohne `END`-Marker, `(?s)-----BEGIN ... PRIVATE KEY-----.*`) konnte dabei
das schließende Fence-Tag mitverschlucken — die Redaction-Richtung blieb
sicher (nur Löschung, nichts wird sichtbar), aber ein kaputter
schließender Fence ist genau die Lücke, durch die eingeschleuste
Anweisungen wieder als vertrauenswürdiger Kontext durchrutschen könnten
(Spec 0039s eigentliche Garantie).

`CommandResult`-Inhalt ist von diesem Problem nicht betroffen: dessen
`stdout`/`stderr` werden redigiert, sobald ein Kommando ausgeführt wird,
und erst **danach**, beim eigentlichen Request-Aufbau in `ai-providers`
(`format_command_result`), gefenct — Redaction läuft dort also bereits
strukturell vor dem Fencing.

## Entscheidung

**Marker-Segmentierung statt eines reinen Reihenfolge-Tauschs.**

Der naheliegende, "eigentlich bevorzugte" Fix — Fencing grundsätzlich erst
beim Versand anwenden, nie vorher, analog zu `CommandResult` — ist für
`RemoteFile`-Inhalt **nicht sauber möglich**, ohne das Persistenzformat zu
ändern: der gefencte String IST die gespeicherte Repräsentation
(`session.context`/die DB enthalten bereits `<remote_file>...
</remote_file>`, nicht getrennt Rohinhalt + Fencing-Metadaten). Ein
wiederaufgenommener Chat redigiert beim nächsten Versand also
zwangsläufig bereits gefencten Text — ein Umbau auf "Fencing erst bei
`send()`" hätte eine eigene, strukturierte `MessageContent`-Repräsentation
für gefencten Inhalt gebraucht (plus Migration bestehender
`chat_messages`-Zeilen) — ein deutlich größerer Eingriff als für einen
"kleinen Fix" angemessen.

Stattdessen: `ssh_manager_core::ai::fence_markers()` liefert die feste
Liste literaler Fence-Marker-Strings, die `fence_untrusted` erzeugen
kann. `orchestration::redact_text_preserving_fence_markers` trennt
bereits gefencten Text an jedem gefundenen Marker auf, redigiert nur die
Segmente *zwischen* zwei Markern unabhängig voneinander und fügt die
Marker unverändert wieder ein — ein Muster kann dadurch nie über eine
Fence-Grenze hinausmatchen, weil Marker und angrenzender Inhalt nie in
derselben an den Redactor übergebenen Zeichenkette stehen. Sicher, weil
`fence_untrusted`s eigenes Escaping (`escape_for_prompt_fence`)
garantiert, dass ein literales `<`/`>` in tatsächlich gefenctem Text nie
aus dem ursprünglichen Inhalt stammt, sondern ausschließlich von den
Fence-Tags selbst.

### Bekannte, bewusst nicht geschlossene Restlücke

`MessageContent::Text` trägt nicht nur gefencten Inhalt, sondern auch
gewöhnlichen, nie escapten Chat-Text (Nutzer-Eingabe, KI-Antworttext über
`flush_text_buffer`). Enthält so ein Text zufällig eine Marker-artige
Teilzeichenkette (z. B. ein zitiertes `</stdout>` ohne jeden echten
Fence-Bezug) **mitten in** einem unterminierten Fail-safe-Treffer (etwa
einem von der KI ausgegebenen, abgeschnittenen Private-Key-Block ohne
`END`-Marker), trennt die Segmentierung die Fail-safe-Reichweite an
dieser Stelle künstlich ab — der Teil hinter dem (zufälligen) Marker
enthält dann keinen `BEGIN`-Header mehr, matcht kein Muster mehr und
bleibt unredigiert. Eine echte, aber eng begrenzte Abschwächung
gegenüber dem Verhalten vor diesem Fix (der bisher den gesamten Text
ungeteilt redigierte).

Geprüft und verworfen:
- **Erst ungeteilt redigieren, nur bei tatsächlich verschlucktem Marker
  auf Segmentierung zurückfallen.** Erwies sich als wirkungslos: genau
  im beschriebenen Fall verschluckt bereits der ungeteilte Durchlauf den
  Marker (das ist ja der Auslöser), der Rückfall landet also exakt in
  derselben Segmentierung wie ohne diese Zusatzstufe — keine
  Verbesserung, nur zusätzliche Komplexität.
- **Fail-safe-Zustand über Segmentgrenzen hinweg mitführen** ("ab hier
  offener Key-Block, Folgesegmente pauschal weiterredigieren"). Würde
  das Problem lösen, bräuchte aber entweder Kenntnis der genauen
  Redactor-Regex-Muster in `orchestration.rs` (Schichten-Verletzung —
  `core::ai::redactor`s Muster sind bewusst nicht als öffentliche API
  exportiert) oder eine Erweiterung des `OutputRedactor`-Traits um
  zustandsbehafteten, segmentübergreifenden Aufruf — unverhältnismäßiger
  Eingriff für dieses eng begrenzte Randproblem.
- **Segmentierung nur bei "wohlgeformtem" Marker-Paar** (öffnender +
  passender schließender Tag) statt bei jedem einzelnen Marker-Fund.
  Schließt den konkreten Testfall des Reviews (ein einzelner, nicht
  gepaarter Marker) — verhindert aber NICHT den allgemeinen Fall: auch
  ein tatsächlich gepaartes `<stdout>...</stdout>` um einen unterminierten
  Key-Block würde denselben Effekt haben (der Key-Body-Abschnitt zwischen
  den beiden Markern enthält für sich genommen keinen `BEGIN`-Header
  mehr). Löst das Problem also strukturell nicht, nur eine seiner
  Erscheinungsformen — als vermeintliche Lösung eher irreführend als eine
  ehrliche Dokumentation der Lücke.

Aus reinem String-Inhalt lässt sich nicht zuverlässig unterscheiden, ob
ein Marker-Vorkommen aus einem echten Fence stammt oder zufällig in nie
gefenctem Text auftaucht — das wäre nur mit einer strukturellen
Kennzeichnung "dieser Text ist gefenct" lösbar, die über die Persistenz
hinweg erhalten bliebe (derselbe größere Umbau, der oben bereits als
unverhältnismäßig verworfen wurde). Die Redaction-Richtung bleibt in
diesem Rand-Rand-Fall weiterhin sicher (nichts wird sichtbar gemacht, was
vorher nicht sichtbar war) — nur die Reichweite ist geringer als im
theoretischen Idealfall. Sollte dieser Rand-Fall künftig relevant werden
(z. B. weil KI-generierter Text real-weltlich private Schlüssel
reproduziert), wäre die strukturelle Kennzeichnung von gefenctem Inhalt
der richtige, größere Schritt — nicht Teil dieses Fixes.

### `UntrustedKind`-Vollständigkeit

`fence_markers()` iteriert über eine explizite Liste aller vier
`UntrustedKind`-Varianten. Eine erste Fassung hielt diese Liste als
separates `UntrustedKind::ALL`-Konstantenarray — der Review fand: eine
neue Variante hätte `tag_name()`s exhaustives `match` zum
Nicht-Kompilieren gebracht, das separate Array aber unbemerkt
unvollständig gelassen, wodurch die neue Variante lautlos ungeschützt
geblieben wäre. Behoben über
`fencing::tests::all_untrusted_kind_variants`: ein echtes, wildcard-freies
`match` auf `UntrustedKind` selbst, das den Build zuverlässig bricht,
sobald eine Variante fehlt (Rusts Exhaustivitätsprüfung ist typbasiert,
nicht datenflussbasiert — sie greift unabhängig davon, welche Werte zur
Laufzeit tatsächlich durch die Schleife laufen).

## Konsequenzen

**Positiv:**
- Der gemeldete Fence-Bruch ist behoben und durch einen Test abgesichert,
  der über den echten `execute_read_remote_file`-Lesepfad geht (nicht nur
  die Historie direkt konstruiert) und ein realistisches "nachträglich
  hinzugefügtes Muster"-Szenario nachstellt (Spec 0040, Abschnitt 5s
  eigene Begründung für die additive Re-Redaction).
- Kein Eingriff ins Persistenzformat, keine Migration bestehender
  `chat_messages`-Zeilen nötig.
- `UntrustedKind`-Vollständigkeit ist jetzt durch einen echten
  Compile-Fehler abgesichert, nicht nur durch Konvention.

**Negativ / Trade-off:**
- Die oben beschriebene Restlücke bleibt bestehen — dokumentiert, bewusst
  nicht geschlossen, aber ein echter (wenn auch eng begrenzter und in der
  sicheren Richtung liegender) Kompromiss gegenüber dem Verhalten vor
  diesem Fix.
- `redact_text_preserving_fence_markers` scannt den Text pro
  `MessageContent::Text`-Nachricht gegen bis zu zehn Marker-Strings — bei
  den hier üblichen Textgrößen (Zeichenbudget aus
  `chat_context_truncation`) unkritisch, aber kein konstanter Aufwand wie
  ein einzelner `redact_text()`-Aufruf.
