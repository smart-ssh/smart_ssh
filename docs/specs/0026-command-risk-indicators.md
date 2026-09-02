# Spec: Risiko-Indikatoren für KI-Vorschläge (Server-Risiko / Daten-Risiko)

Status: Entwurf
Modul: neues Modul `crates/core/src/risk/`, Erweiterung
`crates/app-tauri`, `frontend/`
Abhängigkeiten: Filter-Engine (Spec 0002, Pattern-Typ und
Kommando-Segmentierung werden wiederverwendet), Kernschleife (Spec 0007),
SFTP-Aktionen (Spec 0020), KI-Provider-Verwaltung (Spec 0006/0007),
Einstellungs-Speicher (Spec 0024, `tauri-plugin-store`)

## 1. Ziel

Jeder KI-vorgeschlagenen Aktion (`SuggestCommand`, `ReadRemoteFile`,
`WriteRemoteFile`) wird eine **rein informative** Risiko-Einschätzung auf
zwei unabhängigen Achsen beigefügt:

- **Server-Risiko** (gelb/rot): könnte die Aktion dem Server schaden
  (destruktiv, irreversibel, dienstunterbrechend)?
- **Daten-Risiko** (gelb/rot): könnte die Aktion sensible Daten (Passwörter,
  Schlüssel, Secrets) in den Chatverlauf und damit an den KI-Anbieter
  fließen lassen?

**Kein drittes "grün"-Badge** — Abwesenheit eines Badges bedeutet "laut
bekannten Mustern unauffällig", kein Sicherheitsversprechen.

**Nicht verhandelbar**: Dieser Indikator ersetzt und beeinflusst **nicht**
die Filter-Engine (Spec 0002) — er blockiert nichts, führt nichts automatisch
aus, ändert keine `Decision`. Er ist eine zusätzliche Einschätzung neben der
eigentlichen Freigabe-Entscheidung, sichtbar auch bei `AutoExec`-Kommandos.

## 2. Regelbasierte Basis-Einschätzung

```rust
pub enum RiskLevel { None, Yellow, Red }

pub struct RiskAssessment {
    pub server_risk: RiskLevel,
    pub server_risk_reason: Option<String>,
    pub data_risk: RiskLevel,
    pub data_risk_reason: Option<String>,
    pub ai_reviewed: bool,
}

pub trait RiskClassifier: Send + Sync {
    fn classify(&self, command: &str) -> RiskAssessment;
}
```

Wiederverwendung statt Neubau: Die Kommando-**Segmentierung** bei
Verkettungen (`&&`, `;`, `|`, Command-Substitution) nutzt exakt dieselbe
Logik wie die Filter-Engine (Spec 0002, Abschnitt 4) — jedes Teilkommando
wird einzeln klassifiziert, das Gesamtergebnis je Achse ist das jeweils
höhere Risiko-Level über alle Teile. Die **Muster** selbst nutzen denselben
`Pattern`-Typ (Glob/Regex/Exact, Spec 0002 Abschnitt 2), zwei eigene,
containerinterne Listen statt der Filter-Regeln:

Beispielhafte Server-Risiko-Muster (Rot): `rm -rf *`, `dd if=* of=/dev/*`,
`mkfs*`, Fork-Bomb-Muster, `shutdown*`/`reboot*`/`poweroff*`,
`iptables -F*`, `chmod -R 777 /*`. (Gelb): `rm *` (ohne `-rf`),
`systemctl stop/restart *`, `apt/yum remove *`, `git reset --hard*`,
`kill *` (ohne PID 1).

Beispielhafte Daten-Risiko-Muster (Rot): `cat`/`less`/`head`/`tail` auf
`*id_rsa*`, `*.pem`, `*.key`, `*.env`, `*credentials*`, `*shadow*`,
`*.aws/credentials*`; `env`/`printenv`; `mysqldump`/`pg_dump` ohne
Ziel-Redaction; SQL mit `SELECT * FROM *user*`/`*password*`. (Gelb):
`find` mit `-name *.key`-artigen Mustern, `ls` in `~/.ssh`/`/etc`, `grep`
nach `password`/`secret`/`token` in Dateien.

Diese Listen sind **Startpunkte, kein Anspruch auf Vollständigkeit** — anders
als die Hard-Blacklist der Filter-Engine sind sie bewusst nicht
sicherheitskritisch (sie blockieren nichts), daher ist eine gewisse
Unvollständigkeit tolerierbar und kein Sicherheitsloch, nur eine
unvollständige Warnung. Nutzer-Erweiterbarkeit dieser Listen ist nicht Teil
dieser Spec (siehe offene Punkte).

Für `ReadRemoteFile`/`WriteRemoteFile` (Spec 0020): Klassifizierung läuft
gegen den Dateipfad, gemappt auf dieselben Pseudokommandos
(`sftp-read <pfad>`/`sftp-write <pfad>`), die bereits für die Filter-Engine-
Anbindung in Spec 0020, Abschnitt 4.1 etabliert wurden — dieselbe
Konvention, keine zweite Mapping-Logik.

## 3. Optionale KI-Zweitmeinung (nur Daten-Risiko-Achse)

Bewusst nur für die Daten-Risiko-Achse — semantisches Einordnen ("könnte
dieser Pfad trotz unbekannten Namens sensibel sein") passt besser zu einer
KI-Einschätzung als Server-Schaden, der sich gut musterbasiert erfassen
lässt.

- **Standardmäßig deaktiviert** (Opt-in in den Einstellungen) — das
  Kommando an einen weiteren KI-Anbieter zu schicken ist selbst ein
  zusätzlicher Datenfluss, der explizit gewählt werden muss, nicht
  automatisch passiert.
- **Eigener, separat wählbarer Provider** — Referenz auf eine bestehende
  `AiProviderConfig` (Spec 0007), gespeichert als einfache Einstellung über
  `tauri-plugin-store` (Spec 0024), keine neue SQLite-Tabelle. Empfehlung im
  UI-Hinweistext: bewusst ein lokales Modell (Ollama) wählbar, damit auch
  diese Zweitmeinung nicht zwingend an einen weiteren externen Anbieter
  geht.
- **Minimaler Kontext**: Der Zweitmeinungs-Request bekommt **ausschließlich**
  das Kommando/den Pfad als Text, keinen Chatverlauf, keine Server-Notizen —
  dasselbe Sparsamkeits-Prinzip wie beim `OutputRedactor` (Spec 0006).
  Prompt sinngemäß: "Könnte die Ausgabe dieses Kommandos sensible Daten
  enthalten, die nicht an einen KI-Anbieter weitergegeben werden sollten?
  Antworte nur mit none/yellow/red und einer kurzen Begründung."
- **Nur Eskalation, nie Abschwächung**: Das Endergebnis der Daten-Risiko-
  Achse ist `max(regelbasiertes_ergebnis, ki_ergebnis)`. Eine
  KI-Einschätzung kann ein `None` zu `Yellow` oder `Yellow` zu `Red` anheben,
  aber **niemals** ein regelbasiertes `Red` auf `Yellow`/`None` absenken —
  eine probabilistische Zweitmeinung darf eine deterministische Warnung
  nicht stillschweigend entkräften (auch mit Blick auf mögliche
  Prompt-Injection über den Kommandotext selbst).
- Läuft **asynchron**, nachdem die regelbasierte Einschätzung bereits
  angezeigt wurde — kein Warten auf einen zusätzlichen API-Roundtrip, bevor
  überhaupt ein Badge sichtbar wird.

## 4. Darstellung im UI

- Zwei kleine, getrennte Badges ("Server", "Daten") an der Aktionskarte/dem
  Bestätigungsdialog, nur sichtbar, wenn ein Level ≠ `None` vorliegt.
  Tooltip zeigt den jeweiligen `*_reason`-Text (welches Muster gegriffen
  hat bzw. die KI-Begründung).
- Ist die KI-Zweitmeinung aktiviert und ihr Ergebnis noch ausstehend: ein
  dezenter Lade-Indikator neben dem Daten-Badge (bzw. an dessen Stelle,
  falls die Regel-Einschätzung `None` ergab), der verschwindet oder sich zu
  `Yellow`/`Red` aktualisiert, sobald die Antwort da ist.
- Klarer Hinweistext (z. B. im Tooltip oder als Fußnote): "Einschätzung
  basierend auf bekannten Mustern — keine Garantie." Kein Wort wie "sicher"
  oder "geprüft" ohne Einschränkung, konsistent mit der bereits in Spec
  0025 etablierten Zurückhaltung bei Sicherheits-Aussagen.
- Nur für KI-vorgeschlagene Aktionen — **nicht** für den manuellen
  SFTP-Dateibrowser (Spec 0020, Abschnitt 5) und nicht für direkte
  Terminal-Eingaben, konsistent mit dem Prinzip, dass eigene bewusste
  Aktionen keine Warnung vor sich selbst brauchen.

## 5. Offene Punkte

- Nutzer-Erweiterbarkeit der Muster-Listen (eigene zusätzliche Server-/
  Daten-Risiko-Muster definieren, analog zur Regel-Verwaltung aus Spec
  0009) — sinnvolle spätere Ausbaustufe, nicht Teil dieser Spec.
- Soll die KI-Zweitmeinung optional auch auf die Server-Risiko-Achse
  ausgeweitet werden? Aktuell bewusst nur Daten-Risiko (Abschnitt 3) —
  falls sich in der Praxis zeigt, dass musterbasierte Server-Risiko-Analyse
  zu viele Fälle übersieht, wäre eine Erweiterung denkbar.
