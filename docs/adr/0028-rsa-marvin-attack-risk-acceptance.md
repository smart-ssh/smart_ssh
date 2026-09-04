# 0028-rsa-marvin-attack-risk-acceptance

## Status
Accepted

## Kontext

`docs/specs/0035-ci-dependency-audit.md` führt `cargo audit` als
CI-Prüfung gegen die RustSec-Advisory-Datenbank ein. Der erste lokale Lauf
gegen den aktuellen Workspace-Bestand (2026-09-04) fand genau eine echte
Sicherheitslücke (nicht nur eine "unmaintained"-Warnung):

- **RUSTSEC-2023-0071** — "Marvin Attack": ein Timing-Seitenkanal in der
  `rsa`-Crate (v0.10.0-rc.18), der unter bestimmten Umständen
  Schlüssel-Rückgewinnung ermöglichen kann. Severity 5.9 (medium). Kein
  Fix verfügbar ("No fixed upgrade is available!" laut Advisory).

`rsa` ist keine direkte Abhängigkeit, sondern kommt transitiv über
`russh`/`ssh-key` (`crates/ssh-transport`) — also über die SSH-
Kernbibliothek, die App verwendet RSA-Schlüssel-Operationen aktiv, sobald
ein Nutzer sich mit einem RSA-Schlüssel authentifiziert (`ssh-rsa`,
`rsa-sha2-256`/`512` sind nach wie vor gebräuchliche SSH-Schlüsseltypen).
Dieser Fund wurde dem Nutzer explizit gemeldet (nicht eigenmächtig als
unkritisch eingestuft) und die folgende Entscheidung ist mit ihm
abgestimmt.

## Entscheidung

**Dokumentiertes Risk-Acceptance, kein Ersatz der SSH-Kernbibliothek.**
`RUSTSEC-2023-0071` wird in `audit.toml` mit ausführlicher Begründung
ignoriert, nicht durch einen Bibliothekswechsel behoben.

**Begründung — Bedrohungsmodell:**
Smart SSH ist ein **lokaler Client**. Der private RSA-Schlüssel liegt
lokal auf der Maschine des Nutzers, und die RSA-Signaturoperation (die
laut Advisory über Timing angreifbar sein könnte) läuft ebenfalls lokal.
Der Marvin-Angriff setzt voraus, dass ein Angreifer viele präzise
Zeitmessungen genau dieser Operation von außen vornehmen kann — das
übliche, in der Advisory beschriebene Angriffsszenario ist ein von außen
erreichbarer TLS-Server-Endpunkt, der RSA-Operationen auf eingehende
Anfragen hin ausführt und dessen Antwortzeiten ein entfernter Angreifer
wiederholt messen kann. Für einen lokalen SSH-**Client**, der selbst die
Verbindung initiiert und dessen Signaturoperation nicht als Antwort auf
beliebig oft wiederholbare, extern gesteuerte Anfragen läuft, ist dasselbe
Angriffsszenario strukturell deutlich schwerer bis praktisch nicht
herstellbar.

**Begründung — kein verfügbarer Fix, unverhältnismäßiges Risiko eines
Ersatzes:** Die Advisory selbst nennt keinen Fixed-Upgrade-Pfad. `rsa`
wird nicht direkt, sondern transitiv über `russh` gezogen — ein Ersetzen
der gesamten SSH-Kernbibliothek (`russh`) wäre ein erheblich größerer,
selbst risikobehafteter Eingriff in den sicherheitskritischsten Teil der
App als das verbleibende Risiko dieser einen Advisory.

**Ausdrücklich NICHT Teil dieser Entscheidung:** eine mögliche
Bevorzugung von Ed25519-Schlüsseln in der App (z. B. als empfohlener
Standard-Schlüsseltyp im UI) wurde als Idee genannt, aber bewusst als
eigenständiges, separat zu entscheidendes Thema ausgeklammert — keine
eigenmächtige Änderung an der Auth-Schicht im Rahmen dieses
CI-Härtungsschritts.

## Tracking / Re-Evaluierungs-Auftrag

Diese Ausnahme ist **nicht dauerhaft unbeobachtet** gemeint:

- Sobald `russh` und/oder `rsa` ein Update veröffentlichen, das
  `RUSTSEC-2023-0071` behebt (neue `rsa`-Version ohne die Advisory, oder
  ein `russh`-Update, das eine gefixte `rsa`-Version zieht), ist der
  `Cargo.lock`-Stand zu aktualisieren und der `ignore`-Eintrag in
  `audit.toml` zu entfernen.
- Bis dahin bleibt der CI-Job im Berichtsmodus (`continue-on-error`, s.
  Spec 0035 Abschnitt 5) — sobald er auf blockierend umgestellt wird
  (separater, späterer Schritt), bleibt dieser eine dokumentierte
  `ignore`-Eintrag weiterhin nötig, bis der Fix tatsächlich vorliegt.
- Diese ADR ist der maßgebliche Ort für den aktuellen Stand dieser
  Entscheidung — der Kommentar in `audit.toml` verweist hierher, statt die
  vollständige Begründung an zwei Stellen zu duplizieren.

## Konsequenzen

**Positiv:**
- `cargo audit` kann grün laufen, ohne die Advisory stillschweigend zu
  verstecken — die Ausnahme ist an einer einzigen, für jeden nachvollziehbaren
  Stelle (`audit.toml` + diese ADR) begründet.
- Kein überstürzter, selbst riskanter Wechsel der SSH-Kernbibliothek unter
  Zeitdruck, nur um eine CI-Prüfung grün zu bekommen.

**Negativ / Trade-off:**
- Die zugrundeliegende Schwachstelle in `rsa` bleibt bestehen, bis
  Upstream einen Fix liefert — das Risiko wird bewusst getragen, nicht
  beseitigt. Für das hier beschriebene Bedrohungsmodell (lokaler Client)
  als vertretbar eingeschätzt, aber nicht null.
- Erfordert eine manuelle, künftige Aktion (Dependency-Update +
  `audit.toml`-Bereinigung), die leicht vergessen werden könnte, wenn
  niemand periodisch nachschaut — Spec 0035, Abschnitt 6 nennt
  geplante (nicht nur push-getriggerte) Läufe als offenen Punkt, der genau
  dieses Risiko mindern würde.
