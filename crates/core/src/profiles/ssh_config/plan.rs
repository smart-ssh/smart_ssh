//! Aus geparsten Blöcken einen **Importplan** machen (Spec 0075, §4.1).
//!
//! Der Plan ist die Vorschau (§3.1.7) und gleichzeitig das, was bestätigt
//! ausgeführt wird — dasselbe Objekt, damit Vorschau und Ergebnis nicht
//! auseinanderlaufen können. Dieses Modul legt **nichts** an, öffnet
//! **keine** Datei und kennt kein Tauri.

use std::collections::{BTreeSet, HashMap, HashSet};

use super::parser::{ParsedFile, SkippedKind};
use super::pattern::block_matches;
use crate::filter::{Rule, RuleAction, RuleId, Scope};
use crate::profiles::types::{Group, Server};
use crate::shared::ServerId;

/// Woher ein Wert stammt (§3.1.3: „Die Vorschau zeigt bei jedem betroffenen
/// Feld, aus welchem Block der Wert stammt").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    pub file: String,
    pub line: u32,
    /// Die `Host`-Angaben des Blocks, aus dem der Wert kommt — z. B.
    /// `*.prod.de`. Wörtlich aus der Datei, aber **nur** die Muster; ein
    /// Muster ist kein Fremdinhalt im Sinne von §5.4, sondern wird ohnehin
    /// als Schlagwort angezeigt.
    pub block: String,
}

/// Ein Feldwert mit seiner Herkunft. `origin: None` heißt: Vorgabe des
/// Produkts, nicht aus der Datei (etwa Port 22 nach §3.1.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sourced<T> {
    pub value: T,
    pub origin: Option<Provenance>,
}

/// §4.5: eine Gruppe je gelesener Datei.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedGroup {
    pub name: String,
    /// Index der Elterngruppe in [`ImportPlan::groups`]. Eltern stehen
    /// immer **vor** ihren Kindern (§3.1.11).
    pub parent: Option<usize>,
    pub source_path: String,
}

/// §5.2a: ein Schlagwort, das eine bestehende Filterregel trifft.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchedRule {
    pub rule_id: RuleId,
    pub action: RuleAction,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlannedTag {
    pub tag: String,
    pub origin: Provenance,
    /// Nicht leer ⇒ die Vorschau MUSS das Schlagwort kennzeichnen und die
    /// Regeln nennen (§5.2a).
    pub matched_rules: Vec<MatchedRule>,
    /// `true` ⇒ das Schlagwort traegt **keinen** Platzhalter, stammt also aus
    /// einer buchstaeblichen Angabe in einem gemischten Block
    /// (`Host prod *`).
    ///
    /// §5.2a stuetzt seine Risikoeinschaetzung auf die Annahme, importierte
    /// Schlagworte enthielten **immer** `*`, `?` oder `!` — fuer diesen Fall
    /// trifft das nicht zu, und genau er kann eine bestehende
    /// `Scope::Tag`-Regel **exakt** treffen. Die Vorschau soll ihn deshalb
    /// deutlicher kennzeichnen als ein Mustern-Schlagwort (Review-Runde 1
    /// und 2, offene Entscheidung `Q-BL-0216-02`).
    pub is_literal: bool,
}

/// §3.1.9 (a): Der Pfad wird **unverändert** übernommen — kein `realpath`,
/// keine Existenzprüfung, kein Auflösen von `~` (§4.6, §5.5). Die Datei
/// bleibt hier ungeöffnet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityFilePlan {
    pub path: String,
    pub origin: Provenance,
    /// `false` ⇒ Vorschau kennzeichnet „beim Verbinden nicht benutzbar,
    /// absoluter Pfad nötig" (§3.1.9 a). Ein relativer Pfad und die
    /// `~user/`-Form lehnt Spec 0076 (A-3) beim **Verbinden** ab.
    pub usable_when_connecting: bool,
}

/// Wohin ein `jump_host` zeigt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpTarget {
    /// Ein Eintrag, der in diesem Import entsteht (Index in
    /// [`ImportPlan::entries`]).
    Planned(usize),
    /// Ein Profil, das schon im Bestand liegt.
    Existing(ServerId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictKind {
    /// Gleicher `name` (§3.1.8).
    Name,
    /// Gleiche Kombination aus `host`, `port` und `username` (§3.1.8).
    Address,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub kind: ConflictKind,
    pub existing: ServerId,
    pub existing_name: String,
}

/// Ein Profil, das entstehen würde.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedEntry {
    /// Der `Host`-Alias, wörtlich — gleichzeitig `Server::name` (§4.3).
    pub name: String,
    /// Index in [`ImportPlan::groups`] (§4.5).
    pub group: usize,
    pub host: Sourced<String>,
    pub port: Sourced<u16>,
    pub username: Sourced<String>,
    pub tags: Vec<PlannedTag>,
    pub identity_file: Option<IdentityFilePlan>,
    /// `None` = kein Jump-Host. Wurde ein `ProxyJump` **abgelehnt**, steht
    /// hier `None` und der Grund in [`ImportPlan::skipped`] (§3.1.6).
    pub jump: Option<JumpTarget>,
    pub conflict: Option<Conflict>,
}

/// Warum eine Zeile nicht übernommen wurde. Bewusst ein Typ und kein
/// fertiger Satz: Die Formulierung gehört ins Frontend (i18n), und ein
/// Freitextfeld wäre die Stelle, an der irgendwann doch ein Wert
/// mitwandert (§5.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    /// Direktive, die wir nicht auswerten — auch `Match` (§4.2).
    Unsupported,
    /// Zweiter Wert derselben Direktive im Block (§3.1.9, letzter Absatz).
    AlreadySet,
    /// Wert nach dem Randzeichen-Trim leer (§6.4.7 c).
    EmptyValue,
    /// Wert stand außerhalb eines `Host`-Blocks.
    OutsideHostBlock,
    /// Wert nicht lesbar, etwa ein `Port`, der keine Zahl ist.
    InvalidValue,
    /// Block ohne verwendbaren Alias (§4.3).
    EmptyAlias,
    /// Ein gemischter `Host`-Block (`Host web1 *.de`) gilt ganz als
    /// Platzhalterblock (ADR 0074, Punkt 3); seine buchstäblichen Angaben
    /// ergeben **kein** Profil. Gemeldet statt verschluckt (§3.1.5).
    MixedWildcardBlock,
    /// `Include` jenseits von Tiefe 3 (§3.1.4).
    IncludeTooDeep,
    /// Diese Datei wurde in diesem Import schon gelesen (§3.1.4).
    IncludeAlreadyRead,
    /// Datei nicht lesbar — der Import läuft weiter (§3.1.4).
    IncludeUnreadable,
    /// §3.1.4a: keine `ssh_config`, übersprungen.
    IncludeNotSshConfig,
    /// `Include`-Muster traf keine Datei.
    IncludeNoMatch,
    /// `ProxyJump` abgelehnt (§3.1.6).
    ProxyJump(ProxyJumpRejection),
}

/// Warum eine `ProxyJump`-Kette nicht angelegt wurde (§3.1.6). Jeder Grund
/// wird bei **allen** beteiligten Einträgen gemeldet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyJumpRejection {
    /// Schleife — heute fiele sie erst beim Verbinden auf (§5.3).
    Cycle,
    /// Ein Hop lässt sich weder im Import noch im Bestand auflösen.
    HopUnresolved,
    /// Mehr-Hop-Kette, bei der eine Zwischenstation schon im Bestand
    /// liegt: sie zu bauen hieße, ein fremdes Profil zu ändern (3.1.8).
    IntermediateExists,
    /// Eine Zwischenstation bekäme ihr `jump_host` aus zwei Ketten mit
    /// verschiedenen Vorgängern.
    AmbiguousPredecessor,
    /// Eine Zwischenstation trägt zusätzlich im eigenen Block ein
    /// `ProxyJump` — zweite Quelle, dieselbe Rechtsfolge.
    OwnProxyJumpAndIntermediate,
    /// Zeigt auf den lokalen Pseudo-Server (§5.7,
    /// Code `SERVER_JUMP_HOST_LOCAL`).
    LocalPseudoServer,
}

/// Eine Meldung „nicht übernommen" (§3.1.5) — mit Datei und Zeile, mit dem
/// **Namen** der Direktive, nie mit ihrem Wert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedReport {
    pub file: String,
    pub line: u32,
    pub directive: SkippedKind,
    pub reason: SkipReason,
    /// Bei einem `ProxyJump`-Fund: der betroffene Eintrag, damit die
    /// Vorschau ihn am Profil anzeigen kann.
    pub entry: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ImportPlan {
    /// Eltern vor Kindern (§3.1.11).
    pub groups: Vec<PlannedGroup>,
    /// Jump-Host-Ziele vor ihren Nutzern (§3.1.11).
    pub entries: Vec<PlannedEntry>,
    pub skipped: Vec<SkippedReport>,
}

/// Eine gelesene Datei, wie `app-shell` sie übergibt (§4.1).
#[derive(Debug, Clone)]
pub struct ImportSource {
    /// Voller Pfad — die Vorschau nennt ihn (§3.1.4).
    pub path: String,
    /// Index der einbindenden Datei in derselben Liste; `None` = die
    /// gewählte Datei (Tiefe 0).
    pub parent: Option<usize>,
    pub parsed: ParsedFile,
}

/// Der Bestand, gegen den geplant wird (§4.1). Ohne ihn sind weder
/// Konflikte noch `ProxyJump`-Ziele im Bestand noch Schlagwort-Treffer
/// bestimmbar.
#[derive(Debug, Clone, Copy)]
pub struct Inventory<'a> {
    pub servers: &'a [Server],
    pub groups: &'a [Group],
    pub rules: &'a [Rule],
    /// `ServerId(Uuid::nil())` — hereingegeben statt hier bestimmt, damit
    /// `core` nicht von `app-shell` abhängt (§5.7: Sonderbehandlung nur an
    /// der Grenze, nie in der Sicherheitslogik).
    pub local_server_id: ServerId,
}

/// Ein Hop einer `ProxyJump`-Zeile, aufgelöst.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Hop {
    Planned(usize),
    Existing(ServerId),
    Local,
    Unresolved,
}

struct Chain {
    owner: usize,
    hops: Vec<Hop>,
    file: String,
    line: u32,
}

/// §3.1.6: `user@host:port` auf den reinen Hostnamen zurückführen.
fn hop_host(raw: &str) -> &str {
    // Benutzerteil: OpenSSH trennt am **letzten** `@`, damit ein `@` im
    // Benutzernamen nicht die Adresse zerreißt.
    let after_user = match raw.rfind('@') {
        Some(i) => &raw[i + 1..],
        None => raw,
    };
    // IPv6 in Klammern: `[::1]:22` → `::1`.
    if let Some(rest) = after_user.strip_prefix('[') {
        if let Some(end) = rest.find(']') {
            return &rest[..end];
        }
    }
    // `host:port` nur abschneiden, wenn hinten wirklich eine Zahl steht —
    // sonst verstümmelt es eine nackte IPv6-Adresse.
    if let Some(i) = after_user.rfind(':') {
        let (head, tail) = (&after_user[..i], &after_user[i + 1..]);
        if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) && !head.contains(':') {
            return head;
        }
    }
    after_user
}

/// §3.1.9 (a): Ist der Pfad einer, mit dem sich Spec 0076 später verbinden
/// kann? `/…` ja, `~/…` ja (0076 löst es auf), `~user/…` und alles
/// Relative nein.
fn identity_path_usable(path: &str) -> bool {
    if path.starts_with('/') {
        return true;
    }
    if let Some(rest) = path.strip_prefix('~') {
        return rest.is_empty() || rest.starts_with('/');
    }
    false
}

fn file_stem_name(path: &str) -> String {
    let base = path
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(path);
    base.to_string()
}

/// Baut den Importplan (§4.1).
pub fn build_plan(sources: &[ImportSource], inv: Inventory<'_>) -> ImportPlan {
    let mut plan = ImportPlan::default();

    // ---- §4.5: eine Gruppe je Datei, Eltern vor Kindern -------------
    // `app-shell` liefert die Dateien in Lesereihenfolge, eine eingebundene
    // Datei also immer nach ihrer einbindenden — damit stimmt die
    // Anlegereihenfolge für den Fremdschlüssel `groups.parent_id` ohne
    // weitere Sortierung (§3.1.11).
    let mut used_group_names: HashSet<String> = inv.groups.iter().map(|g| g.name.clone()).collect();
    let mut group_of_file: Vec<usize> = Vec::with_capacity(sources.len());
    for (i, src) in sources.iter().enumerate() {
        let wanted = file_stem_name(&src.path);
        // §4.5: Kollidiert der Name, entsteht eine **neue** Gruppe mit
        // angehängter Nummer, statt in eine bestehende hineinzuschreiben —
        // sonst vermischen sich zwei Importe unbemerkt. Auch gegen die
        // Namen dieses Laufs, damit zwei gleichnamige Dateien aus
        // verschiedenen Verzeichnissen unterscheidbar bleiben.
        let mut name = wanted.clone();
        let mut n = 2usize;
        while used_group_names.contains(&name) {
            name = format!("{wanted} {n}");
            n += 1;
        }
        used_group_names.insert(name.clone());
        let parent = src.parent.map(|p| group_of_file[p]);
        debug_assert!(src.parent.is_none_or(|p| p < i), "Eltern zuerst");
        group_of_file.push(plan.groups.len());
        plan.groups.push(PlannedGroup {
            name,
            parent,
            source_path: src.path.clone(),
        });
    }

    // ---- Die Meldungen des Parsers übernehmen (§3.1.5) ---------------
    for src in sources {
        for s in &src.parsed.skipped {
            plan.skipped.push(SkippedReport {
                file: src.path.clone(),
                line: s.line,
                directive: s.kind.clone(),
                // Der Parser trennt die Fälle nicht; „nicht ausgewertet"
                // ist für die Anzeige dieselbe Aussage. Der Grund, der
                // wirklich zählt, ist bei `ProxyJump` — und den setzen wir
                // unten selbst.
                reason: SkipReason::Unsupported,
                entry: None,
            });
        }
    }

    // ---- Konkrete Aliase sammeln, Reihenfolge = Lesereihenfolge ------
    struct Candidate {
        alias: String,
        group: usize,
    }
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut seen_alias: HashMap<String, usize> = HashMap::new();
    for (fi, src) in sources.iter().enumerate() {
        for block in &src.parsed.blocks {
            if block.is_wildcard() {
                // Ein **gemischter** Block (`Host web1 *.de`) gilt ganz als
                // Platzhalterblock (ADR 0074, Punkt 3). Seine
                // buchstäblichen Angaben ergeben deshalb kein Profil — aber
                // das darf nicht stillschweigend geschehen (§3.1.5,
                // „Stillschweigend verschlucken bleibt ausgeschlossen"):
                // Der Nutzer, der `web1` erwartet hat, bekäme sonst weder
                // ein Profil noch einen Hinweis (Review-Runde 1).
                if block.clauses.iter().any(|c| !c.negated && !c.is_wildcard()) {
                    plan.skipped.push(SkippedReport {
                        file: src.path.clone(),
                        line: block.line,
                        directive: SkippedKind::Directive("Host".to_string()),
                        reason: SkipReason::MixedWildcardBlock,
                        entry: None,
                    });
                }
                continue;
            }
            let mut any = false;
            for clause in &block.clauses {
                // §4.3: leerer Alias ⇒ kein Profil, Block gemeldet.
                if clause.pattern.is_empty() {
                    continue;
                }
                any = true;
                if seen_alias.contains_key(&clause.pattern) {
                    continue;
                }
                seen_alias.insert(clause.pattern.clone(), candidates.len());
                candidates.push(Candidate {
                    alias: clause.pattern.clone(),
                    group: group_of_file[fi],
                });
            }
            if !any {
                plan.skipped.push(SkippedReport {
                    file: src.path.clone(),
                    line: block.line,
                    directive: SkippedKind::Directive("Host".to_string()),
                    reason: SkipReason::EmptyAlias,
                    entry: None,
                });
            }
        }
    }

    // ---- Felder auflösen: „der erste gewinnt" (§3.1.3) ---------------
    // Für jeden Alias alle Blöcke in Lesereihenfolge durchgehen; der
    // konkrete Block ist selbst einer davon. Wer hier nach Blocktyp
    // sortieren würde, brach die OpenSSH-Regel aus §6.1.8.
    let mut entries: Vec<PlannedEntry> = Vec::with_capacity(candidates.len());
    // Je Eintrag der rohe `ProxyJump`-Wert — aufgelöst wird erst, wenn
    // alle Einträge stehen (§3.1.6, „über alle Dateien hinweg").
    let mut proxy_raw: Vec<Option<(String, Provenance)>> = Vec::with_capacity(candidates.len());
    for cand in &candidates {
        let alias = &cand.alias;
        let mut host: Option<(String, Provenance)> = None;
        let mut port: Option<(u16, Provenance)> = None;
        let mut user: Option<(String, Provenance)> = None;
        let mut proxy: Option<(String, Provenance)> = None;
        let mut ident: Option<(String, Provenance)> = None;
        let mut tags: Vec<PlannedTag> = Vec::new();
        let mut tag_seen: BTreeSet<String> = BTreeSet::new();

        for src in sources.iter() {
            for block in &src.parsed.blocks {
                if !block_matches(&block.clauses, alias) {
                    continue;
                }
                let label = block
                    .clauses
                    .iter()
                    .map(|c| {
                        if c.negated {
                            format!("!{}", c.pattern)
                        } else {
                            c.pattern.clone()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                let prov = |line: u32| Provenance {
                    file: src.path.clone(),
                    line,
                    block: label.clone(),
                };

                if let (None, Some(v)) = (&host, &block.host_name) {
                    host = Some((v.value.clone(), prov(v.line)));
                }
                if let (None, Some(v)) = (&user, &block.user) {
                    user = Some((v.value.clone(), prov(v.line)));
                }
                if let (None, Some(v)) = (&proxy, &block.proxy_jump) {
                    proxy = Some((v.value.clone(), prov(v.line)));
                }
                if let (None, Some(v)) = (&ident, &block.identity_file) {
                    ident = Some((v.value.clone(), prov(v.line)));
                }
                if port.is_none() {
                    if let Some(v) = &block.port {
                        match v.value.parse::<u16>() {
                            Ok(p) if p > 0 => port = Some((p, prov(v.line))),
                            _ => {
                                // Nicht stillschweigend auf 22 fallen:
                                // sichtbar melden (§3.1.5) und die Vorgabe
                                // nehmen.
                                plan.skipped.push(SkippedReport {
                                    file: src.path.clone(),
                                    line: v.line,
                                    directive: SkippedKind::Directive("Port".to_string()),
                                    reason: SkipReason::InvalidValue,
                                    entry: Some(alias.clone()),
                                });
                            }
                        }
                    }
                }

                // §3.1.3: Platzhalterblock ⇒ Schlagwort, wörtlich.
                if block.is_wildcard() {
                    for c in &block.clauses {
                        if c.negated || c.pattern == "*" {
                            // `Host *` trägt keine Information (§3.1.3).
                            continue;
                        }
                        // **Hier stand in Review-Runde 1 ein
                        // `if !c.is_wildcard() { continue; }`** — und der
                        // Lockerungs-Gegencheck (Runde 2) hat gezeigt, dass
                        // das eine Verschlechterung war, keine Verbesserung:
                        //
                        // Der Fund aus Runde 1 war, dass ein gemischter Block
                        // (`Host prod *`) dem Server `prod` das
                        // buchstäbliche Schlagwort `prod` gibt und damit eine
                        // bestehende Tag-**Allow**-Regel exakt treffen kann
                        // (`Confirm` → `Allow`). Das Schlagwort wegzulassen
                        // schließt diesen Weg — nimmt aber demselben Profil
                        // auch die Abdeckung durch eine bestehende
                        // Tag-**Deny**-Regel. Eine Verengung in der einen
                        // Richtung ist hier eine Lockerung in der anderen,
                        // und `Deny` nicht mehr greifen zu lassen ist die
                        // teurere Hälfte („Eskalation nur in eine Richtung",
                        // ADR 0024).
                        //
                        // Deshalb bleibt das Schlagwort, und der Weg wird
                        // dort geschlossen, wo §5.2a ihn ohnehin schließen
                        // will: Es wird gekennzeichnet (`is_literal`) und ist
                        // in der Vorschau einzeln abwählbar. Q-BL-0216-02
                        // (Stefan, 2026-09-25, §9 der Spec) hat entschieden,
                        // welche der beiden Richtungen die Vorgabe trägt:
                        // Trifft das buchstäbliche Schlagwort eine
                        // Allow-Regel, ist es in der Vorschau standardmäßig
                        // abgewählt (umgesetzt in `defaultTagSelected`,
                        // `SshConfigImportDialog.tsx`) — s. ADR 0074 Punkt 8,
                        // ADR 0075 §9.
                        if !c.matches(alias) || tag_seen.contains(&c.pattern) {
                            continue;
                        }
                        tag_seen.insert(c.pattern.clone());
                        // §5.2a: trifft das Schlagwort eine bestehende Regel?
                        let matched_rules = inv
                            .rules
                            .iter()
                            .filter(|r| matches!(&r.scope, Scope::Tag(t) if t == &c.pattern))
                            .map(|r| MatchedRule {
                                rule_id: r.id.clone(),
                                action: r.action.clone(),
                            })
                            .collect();
                        tags.push(PlannedTag {
                            is_literal: !c.is_wildcard(),
                            tag: c.pattern.clone(),
                            origin: prov(block.line),
                            matched_rules,
                        });
                    }
                }
            }
        }

        let identity_file = ident.map(|(path, origin)| IdentityFilePlan {
            usable_when_connecting: identity_path_usable(&path),
            path,
            origin,
        });

        entries.push(PlannedEntry {
            name: alias.clone(),
            group: cand.group,
            // §3.1.2: fehlt `HostName`, gilt der Alias als Adresse.
            host: match host {
                Some((v, o)) => Sourced {
                    value: v,
                    origin: Some(o),
                },
                None => Sourced {
                    value: alias.clone(),
                    origin: None,
                },
            },
            // §3.1.2: fehlt `Port`, gilt 22.
            port: match port {
                Some((v, o)) => Sourced {
                    value: v,
                    origin: Some(o),
                },
                None => Sourced {
                    value: 22,
                    origin: None,
                },
            },
            // §3.1.2: fehlt `User`, bleibt er leer.
            username: match user {
                Some((v, o)) => Sourced {
                    value: v,
                    origin: Some(o),
                },
                None => Sourced {
                    value: String::new(),
                    origin: None,
                },
            },
            tags,
            identity_file,
            jump: None,
            conflict: None,
        });

        // `proxy` wird unten in einem eigenen Durchgang aufgelöst — erst
        // wenn alle Einträge stehen, ist „Ziel entsteht in diesem Import"
        // überhaupt beantwortbar (§3.1.6, „über alle Dateien hinweg").
        proxy_raw.push(proxy);
    }

    // ---- §3.1.6: ProxyJump ------------------------------------------
    resolve_proxy_jumps(&mut plan, &mut entries, &proxy_raw, inv);

    // ---- §3.1.8: Konflikte ------------------------------------------
    for e in entries.iter_mut() {
        e.conflict = find_conflict(e, inv.servers);
    }

    // ---- §3.1.11: Jump-Ziele vor ihren Nutzern ------------------------
    plan.entries = order_entries(entries);
    plan
}

fn find_conflict(e: &PlannedEntry, servers: &[Server]) -> Option<Conflict> {
    if let Some(s) = servers.iter().find(|s| s.name == e.name) {
        return Some(Conflict {
            kind: ConflictKind::Name,
            existing: s.id,
            existing_name: s.name.clone(),
        });
    }
    servers
        .iter()
        .find(|s| {
            s.host == e.host.value && s.port == e.port.value && s.username == e.username.value
        })
        .map(|s| Conflict {
            kind: ConflictKind::Address,
            existing: s.id,
            existing_name: s.name.clone(),
        })
}

/// §3.1.6 in voller Länge: Hops auflösen, Kanten in der **richtigen
/// Richtung** bauen, die zwei harten Bedingungen prüfen, Schleifen
/// ablehnen — und jede Ablehnung bei **allen** beteiligten Einträgen
/// melden, nicht gekürzt und nicht teilweise angelegt.
fn resolve_proxy_jumps(
    plan: &mut ImportPlan,
    entries: &mut [PlannedEntry],
    proxy_raw: &[Option<(String, Provenance)>],
    inv: Inventory<'_>,
) {
    let by_alias: HashMap<&str, usize> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| (e.name.as_str(), i))
        .collect();

    let mut chains: Vec<Chain> = Vec::new();
    for (i, raw) in proxy_raw.iter().enumerate() {
        let Some((value, prov)) = raw else { continue };
        // §3.1.6: `ProxyJump none` ⇒ kein Jump-Host, und das ist eine
        // Aussage, keine Auslassung — also auch keine Meldung.
        if value.trim().eq_ignore_ascii_case("none") {
            continue;
        }
        let hops: Vec<Hop> = value
            .split(',')
            .map(|h| resolve_hop(h.trim(), &by_alias, inv))
            .collect();
        chains.push(Chain {
            owner: i,
            hops,
            file: prov.file.clone(),
            line: prov.line,
        });
    }

    let mut rejected: HashMap<usize, ProxyJumpRejection> = HashMap::new();

    // --- Bedingung 0: jeder Hop muss auflösbar und nicht der lokale
    // Pseudo-Server sein (§3.1.6, §5.7).
    for (ci, ch) in chains.iter().enumerate() {
        if ch.hops.is_empty() || ch.hops.iter().any(|h| matches!(h, Hop::Local)) {
            rejected.insert(ci, ProxyJumpRejection::LocalPseudoServer);
        }
        if ch.hops.is_empty() {
            rejected.insert(ci, ProxyJumpRejection::HopUnresolved);
        }
    }
    for (ci, ch) in chains.iter().enumerate() {
        if rejected.contains_key(&ci) {
            continue;
        }
        if ch.hops.iter().any(|h| matches!(h, Hop::Unresolved)) {
            rejected.insert(ci, ProxyJumpRejection::HopUnresolved);
        }
    }

    // --- Bedingung 1 (§3.1.6): Bei einer Mehr-Hop-Kette müssen **alle**
    // Zwischenstationen in diesem Import neu entstehen. Liegt eine schon im
    // Bestand, hieße die Kette zu bauen, ein fremdes Profil zu ändern.
    // Ein **einzelner** Hop darf dagegen auf den Bestand zeigen — dort wird
    // nur `owner.jump_host` gesetzt, kein fremdes Profil angefasst.
    for (ci, ch) in chains.iter().enumerate() {
        if rejected.contains_key(&ci) || ch.hops.len() < 2 {
            continue;
        }
        if ch.hops.iter().any(|h| matches!(h, Hop::Existing(_))) {
            rejected.insert(ci, ProxyJumpRejection::IntermediateExists);
        }
    }

    // --- Bedingung 2 (§3.1.6): Für das `jump_host` einer Zwischenstation
    // gibt es **genau eine** Quelle. Zwei Wege dorthin, beide zählen:
    // verschiedene Vorgänger aus zwei Ketten, oder ein eigenes `ProxyJump`
    // im eigenen Block. Fixpunkt, weil eine Ablehnung Kanten wegnimmt und
    // damit eine andere Mehrdeutigkeit auflösen kann.
    loop {
        let mut incoming: HashMap<usize, Vec<(usize, Hop)>> = HashMap::new();
        for (ci, ch) in chains.iter().enumerate() {
            if rejected.contains_key(&ci) {
                continue;
            }
            for k in (1..ch.hops.len()).rev() {
                if let Hop::Planned(t) = ch.hops[k] {
                    incoming.entry(t).or_default().push((ci, ch.hops[k - 1]));
                }
            }
        }

        let mut newly: Vec<(usize, ProxyJumpRejection)> = Vec::new();
        for (target, srcs) in &incoming {
            let distinct: HashSet<Hop> = srcs.iter().map(|(_, p)| *p).collect();
            if distinct.len() > 1 {
                for (ci, _) in srcs {
                    newly.push((*ci, ProxyJumpRejection::AmbiguousPredecessor));
                }
            }
            // Zweite Quelle: die Zwischenstation trägt im eigenen Block ein
            // `ProxyJump` (§3.1.6, Bedingung 2, zweiter Fall).
            if let Some(own_ci) = chains.iter().position(|c| c.owner == *target) {
                if !rejected.contains_key(&own_ci) {
                    newly.push((own_ci, ProxyJumpRejection::OwnProxyJumpAndIntermediate));
                    for (ci, _) in srcs {
                        newly.push((*ci, ProxyJumpRejection::OwnProxyJumpAndIntermediate));
                    }
                }
            }
        }
        let before = rejected.len();
        for (ci, why) in newly {
            rejected.entry(ci).or_insert(why);
        }
        if rejected.len() == before {
            break;
        }
    }

    // --- Kanten der überlebenden Ketten (§3.1.6, Kantenrichtung!) -------
    // Für `Host x` mit `ProxyJump a,b,c`: x→c, c→b, b→a, und `a` bleibt
    // ohne `jump_host`. Wer hier `hops[0]` an den Besitzer hängt, baut die
    // Kette seitenverkehrt — §6.4.4a (a) prüft jede Kante einzeln.
    let mut jump: HashMap<usize, Hop> = HashMap::new();
    let mut edge_chain: HashMap<usize, usize> = HashMap::new();
    let fill = |rejected: &HashMap<usize, ProxyJumpRejection>,
                jump: &mut HashMap<usize, Hop>,
                edge_chain: &mut HashMap<usize, usize>| {
        jump.clear();
        edge_chain.clear();
        for (ci, ch) in chains.iter().enumerate() {
            if rejected.contains_key(&ci) {
                continue;
            }
            let n = ch.hops.len();
            jump.insert(ch.owner, ch.hops[n - 1]);
            edge_chain.insert(ch.owner, ci);
            for k in (1..n).rev() {
                if let Hop::Planned(t) = ch.hops[k] {
                    jump.insert(t, ch.hops[k - 1]);
                    edge_chain.insert(t, ci);
                }
            }
        }
    };
    fill(&rejected, &mut jump, &mut edge_chain);

    // --- Schleifen (§3.1.6, §5.3): Der Import zieht die Prüfung vor, die
    // heute erst beim Verbinden greift (`jump_host.rs:27-29`). Nur
    // geplante Kanten können eine Schleife bilden: ein Profil im Bestand
    // kann nicht auf ein Profil zeigen, das es noch nicht gibt.
    loop {
        let mut found: Option<Vec<usize>> = None;
        'outer: for start in 0..entries.len() {
            let mut seen: Vec<usize> = Vec::new();
            let mut cur = start;
            loop {
                if seen.contains(&cur) {
                    let at = seen.iter().position(|&x| x == cur).unwrap();
                    found = Some(seen[at..].to_vec());
                    break 'outer;
                }
                seen.push(cur);
                match jump.get(&cur) {
                    Some(Hop::Planned(next)) => cur = *next,
                    _ => break,
                }
            }
        }
        let Some(cycle) = found else { break };
        for node in cycle {
            if let Some(ci) = edge_chain.get(&node) {
                rejected.entry(*ci).or_insert(ProxyJumpRejection::Cycle);
            }
        }
        fill(&rejected, &mut jump, &mut edge_chain);
    }

    // --- Übernehmen ---------------------------------------------------
    for (i, e) in entries.iter_mut().enumerate() {
        e.jump = match jump.get(&i) {
            Some(Hop::Planned(t)) => Some(JumpTarget::Planned(*t)),
            Some(Hop::Existing(id)) => Some(JumpTarget::Existing(*id)),
            _ => None,
        };
    }

    // --- Melden: bei **allen** beteiligten Einträgen (§3.1.6) ----------
    for (ci, ch) in chains.iter().enumerate() {
        let Some(why) = rejected.get(&ci) else {
            continue;
        };
        let mut involved: Vec<usize> = vec![ch.owner];
        for h in &ch.hops {
            if let Hop::Planned(t) = h {
                if !involved.contains(t) {
                    involved.push(*t);
                }
            }
        }
        for idx in involved {
            plan.skipped.push(SkippedReport {
                file: ch.file.clone(),
                line: ch.line,
                directive: SkippedKind::Directive("ProxyJump".to_string()),
                reason: SkipReason::ProxyJump(*why),
                entry: Some(entries[idx].name.clone()),
            });
        }
    }
}

fn resolve_hop(raw: &str, by_alias: &HashMap<&str, usize>, inv: Inventory<'_>) -> Hop {
    let h = hop_host(raw);
    if h.is_empty() {
        return Hop::Unresolved;
    }
    // §3.1.6: erst die Einträge dieses Imports (über alle Dateien hinweg),
    // dann der Bestand.
    if let Some(&i) = by_alias.get(h) {
        return Hop::Planned(i);
    }
    // Im Bestand erst über den Namen, dann über die Adresse — in einer
    // `ssh_config` steht in `ProxyJump` üblicherweise ein Alias.
    let found = inv
        .servers
        .iter()
        .find(|s| s.name == h)
        .or_else(|| inv.servers.iter().find(|s| s.host == h));
    match found {
        // §5.7 / §6.4.8: Der lokale Pseudo-Server ist kein SSH-Ziel.
        Some(s) if s.id == inv.local_server_id => Hop::Local,
        Some(s) => Hop::Existing(s.id),
        None => Hop::Unresolved,
    }
}

/// §3.1.11: Jump-Host-**Ziele** vor den Profilen, die auf sie zeigen —
/// `servers.jump_host_id` ist ein Fremdschlüssel (§1.4). Die Indizes in
/// [`JumpTarget::Planned`] werden dabei mitgezogen.
fn order_entries(entries: Vec<PlannedEntry>) -> Vec<PlannedEntry> {
    let n = entries.len();
    let mut placed = vec![false; n];
    let mut order: Vec<usize> = Vec::with_capacity(n);
    let mut progress = true;
    while progress && order.len() < n {
        progress = false;
        for i in 0..n {
            if placed[i] {
                continue;
            }
            let ready = match entries[i].jump {
                Some(JumpTarget::Planned(t)) => placed[t],
                _ => true,
            };
            if ready {
                placed[i] = true;
                order.push(i);
                progress = true;
            }
        }
    }
    // Sicherheitsnetz: Nach der Zyklusprüfung darf hier nichts übrig
    // bleiben. Bliebe doch etwas, wird es angehängt statt verschluckt —
    // sichtbar falsch ist besser als still verschwunden.
    debug_assert!(order.len() == n, "Zyklus trotz Prüfung in 3.1.6");
    for (i, done) in placed.iter().enumerate() {
        if !done {
            order.push(i);
        }
    }

    let mut new_index = vec![0usize; n];
    for (new, &old) in order.iter().enumerate() {
        new_index[old] = new;
    }
    let mut slots: Vec<Option<PlannedEntry>> = entries.into_iter().map(Some).collect();
    let mut out = Vec::with_capacity(n);
    for &old in &order {
        let mut e = slots[old].take().expect("jeder Index genau einmal");
        if let Some(JumpTarget::Planned(t)) = e.jump {
            e.jump = Some(JumpTarget::Planned(new_index[t]));
        }
        out.push(e);
    }
    out
}
