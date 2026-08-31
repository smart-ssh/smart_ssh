use std::cmp::Ordering;

use async_trait::async_trait;

use super::blacklist;
use super::parser::{self, ParseResult};
use super::types::{
    Decision, EffectiveScope, EvalContext, EvaluationTrace, Rule, RuleAction, RuleId, Scope,
};

/// Ab dieser Zeichenlänge wird ein Kommando ungeprüft auf `Confirm` gesetzt,
/// statt es vollständig zu parsen (Spec 0002, Abschnitt 6, Testfall 11).
/// Über [`FilterEngine::with_max_command_length`] konfigurierbar.
pub const DEFAULT_MAX_COMMAND_LENGTH: usize = 4096;

/// Quelle für die auf einen [`EffectiveScope`] anwendbaren [`Rule`]s (Spec
/// 0002, Abschnitt 5).
///
/// Als Trait modelliert, damit Tests eine In-Memory-Implementierung nutzen
/// können, ohne von der späteren Datenbank-Anbindung abzuhängen.
///
/// `async fn` (Spec 0009, Abschnitt 2): `SqlitePolicyStore` liest die
/// Regeln aus der SQLite-Datenbank, was I/O ist — dasselbe Vorgehen wie
/// beim `ProfileStore`-Umbau in Spec 0004 (`async-trait`, Trait-Objekt bleibt
/// über `Box`/`Arc<dyn ...>` nutzbar).
#[async_trait]
pub trait PolicyStore {
    async fn rules_for(&self, scope: &EffectiveScope) -> Vec<Rule>;
}

/// Wertet Kommandos gegen die Hard-Blacklist und die vom [`PolicyStore`]
/// gelieferten Nutzerregeln aus (Spec 0002, Abschnitt 5).
pub struct FilterEngine<S: PolicyStore> {
    store: S,
    max_command_length: usize,
}

impl<S: PolicyStore> FilterEngine<S> {
    pub fn new(store: S) -> Self {
        Self {
            store,
            max_command_length: DEFAULT_MAX_COMMAND_LENGTH,
        }
    }

    pub fn with_max_command_length(store: S, max_command_length: usize) -> Self {
        Self {
            store,
            max_command_length,
        }
    }

    /// Voller Präzedenz-Ablauf aus Spec Abschnitt 3: Hard-Blacklist → Deny →
    /// Confirm → Allow → Default Confirm, angewandt auf jedes Teilkommando
    /// (Abschnitt 4), strengstes Ergebnis gewinnt.
    ///
    /// Ruft intern [`Self::evaluate_explained`] auf und behält nur die
    /// `Decision` — Spec 0009, Abschnitt 4: "Diese Methode [`evaluate_explained`]
    /// wird nicht im eigentlichen KI-Kommando-Loop verwendet". Die eigentliche
    /// Auswertungslogik existiert dadurch nur einmal statt zweimal.
    pub async fn evaluate(&self, command: &str, ctx: &EvalContext) -> Decision {
        self.evaluate_explained(command, ctx).await.decision
    }

    /// Wie [`Self::evaluate`], liefert aber zusätzlich eine nachvollziehbare
    /// [`EvaluationTrace`] (Spec 0009, Abschnitt 4). Ursprünglich
    /// ausschließlich für die Testen-Funktion im UI gedacht — wird seit Spec
    /// 0016 Abschnitt 4 Punkt 4 auch von [`Self::evaluate`] selbst über den
    /// gemeinsamen Rückgabewert genutzt, um jede Filter-Entscheidung der
    /// Kernschleife strukturiert zu loggen (Kommando, `Decision`, gegriffene
    /// Regel/Hard-Blacklist-Eintrag), nicht nur der expliziten
    /// "Testen"-Ansicht.
    pub async fn evaluate_explained(&self, command: &str, ctx: &EvalContext) -> EvaluationTrace {
        let trace = self.evaluate_explained_inner(command, ctx).await;
        tracing::info!(
            command,
            decision = ?trace.decision,
            matched_rule = ?trace.matched_rule,
            matched_hard_blacklist_entry = ?trace.matched_hard_blacklist_entry,
            "filter engine decision",
        );
        trace
    }

    async fn evaluate_explained_inner(&self, command: &str, ctx: &EvalContext) -> EvaluationTrace {
        if command.trim().is_empty() {
            // Testfall 10: leerer/reiner Whitespace-String -> Deny, nicht nur
            // Confirm, da es schlicht kein Kommando gibt, das ausgeführt
            // werden könnte.
            return EvaluationTrace {
                decision: Decision::Deny {
                    reason: "kein sinnvolles Kommando (leere Eingabe)".to_string(),
                },
                matched_rule: None,
                matched_hard_blacklist_entry: None,
                sub_command_traces: Vec::new(),
            };
        }
        if command.chars().count() > self.max_command_length {
            // Testfall 11: bewusst VOR jedem Parsing/Blacklist-Scan geprüft,
            // damit extrem lange Payloads gar nicht erst vollständig
            // analysiert werden müssen.
            return EvaluationTrace {
                decision: Decision::Confirm {
                    reason: format!(
                        "Kommando überschreitet Längenlimit von {} Zeichen",
                        self.max_command_length
                    ),
                },
                matched_rule: None,
                matched_hard_blacklist_entry: None,
                sub_command_traces: Vec::new(),
            };
        }

        let scope = EffectiveScope::from(ctx);
        let rules = self.store.rules_for(&scope).await;
        self.evaluate_parsed_explained(command, &rules)
    }

    /// Zerlegt `command` in Teilkommandos und wertet sie aus. Bei genau
    /// einem Teilkommando (der Normalfall, kein Chaining) wird dessen Trace
    /// direkt zurückgegeben statt in einen redundanten Wrapper-Trace
    /// eingepackt — `matched_rule`/`matched_hard_blacklist_entry` bleiben so
    /// auch für einzelne Kommandos aussagekräftig. Erst bei **mehreren**
    /// Teilkommandos (echtes Chaining, Spec 0002 Abschnitt 4) entsteht ein
    /// Wrapper-Trace ohne eigenes `matched_rule` (das wäre über mehrere,
    /// potenziell unterschiedliche Teilkommandos hinweg nicht mehr
    /// eindeutig), aber mit einem `sub_command_traces`-Eintrag pro
    /// Teilkommando — genau das macht Spec 0009 Abschnitt 6 für die
    /// Testen-Anzeige ("jeder Teil einzeln ... plus die Gesamt-Entscheidung").
    fn evaluate_parsed_explained(&self, command: &str, rules: &[Rule]) -> EvaluationTrace {
        match parser::split_command(command) {
            ParseResult::Empty => EvaluationTrace {
                decision: Decision::Deny {
                    reason: "kein sinnvolles Kommando (leere Eingabe)".to_string(),
                },
                matched_rule: None,
                matched_hard_blacklist_entry: None,
                sub_command_traces: Vec::new(),
            },
            ParseResult::Ambiguous { reason } => EvaluationTrace {
                decision: Decision::Confirm { reason },
                matched_rule: None,
                matched_hard_blacklist_entry: None,
                sub_command_traces: Vec::new(),
            },
            ParseResult::Segments(segments) if segments.len() == 1 => {
                self.evaluate_segment_explained(&segments[0], rules)
            }
            ParseResult::Segments(segments) => {
                let sub_command_traces: Vec<EvaluationTrace> = segments
                    .iter()
                    .map(|segment| self.evaluate_segment_explained(segment, rules))
                    .collect();
                let decision = sub_command_traces
                    .iter()
                    .map(|trace| trace.decision.clone())
                    .fold(Decision::AutoExec, combine);
                EvaluationTrace {
                    decision,
                    matched_rule: None,
                    matched_hard_blacklist_entry: None,
                    sub_command_traces,
                }
            }
        }
    }

    /// Wertet genau ein Teilkommando aus — Hard-Blacklist, Nutzerregeln und
    /// rekursiv jede darin gefundene Command-Substitution (als
    /// `sub_command_traces`, s. `EvaluationTrace`-Doc-Kommentar).
    fn evaluate_segment_explained(&self, raw_segment: &str, rules: &[Rule]) -> EvaluationTrace {
        let normalized = parser::normalize_whitespace(raw_segment);
        // `elevated` wird bewusst nicht in die öffentliche `Decision`
        // geschrieben (die laut Spec Abschnitt 5 exakt AutoExec/Confirm/Deny
        // ist) — siehe Kommentar bei `evaluate_rules_explained` dazu, wie das
        // Elevated-Wissen stattdessen ins Matching einfließt.
        let (_elevated, stripped) = parser::detect_elevation(&normalized);

        let (original_literal, _) = parser::strip_substitutions(&normalized);
        let (stripped_literal, inner_contents) = parser::strip_substitutions(&stripped);
        let original_literal = parser::normalize_whitespace(&original_literal);
        let stripped_literal = parser::normalize_whitespace(&stripped_literal);

        let matched_hard_blacklist_entry = blacklist::matching_entry(&original_literal)
            .or_else(|| blacklist::matching_entry(&stripped_literal));
        let blacklist_decision = if matched_hard_blacklist_entry.is_some() {
            Decision::Confirm {
                reason: "Hard-Blacklist: potenziell gefährliches Kommando".to_string(),
            }
        } else {
            Decision::AutoExec
        };

        let (rule_decision, matched_rule) =
            evaluate_rules_explained(rules, &original_literal, &stripped_literal);

        let sub_command_traces: Vec<EvaluationTrace> = inner_contents
            .into_iter()
            .map(|inner| self.evaluate_parsed_explained(&inner, rules))
            .collect();
        let substitution_decision = if sub_command_traces.is_empty() {
            Decision::AutoExec
        } else {
            let inner_decision = sub_command_traces
                .iter()
                .map(|trace| trace.decision.clone())
                .fold(Decision::AutoExec, combine);
            combine(
                inner_decision,
                Decision::Confirm {
                    reason: "Command-Substitution ($(...) / `...`) erkannt".to_string(),
                },
            )
        };

        let decision = combine(
            combine(rule_decision, substitution_decision),
            blacklist_decision,
        );

        EvaluationTrace {
            decision,
            matched_rule,
            matched_hard_blacklist_entry,
            sub_command_traces,
        }
    }
}

/// Prüft `rules` gemäß der Bucket-Reihenfolge aus Spec Abschnitt 3
/// (Deny-Regeln → Confirm-Regeln → Allow-Regeln, je nach Scope-Spezifität
/// Server > Tag > Global, dann `priority` absteigend). Der erste Treffer in
/// einem Bucket entscheidet; matcht kein Bucket, greift der Default
/// (`Confirm { reason: "keine Regel gefunden" }`).
///
/// Design-Entscheidung (nicht explizit in Abschnitt 7 der Spec, aber direkt
/// aus Abschnitt 4.6 und Testfall 7 folgend): ein Muster wird sowohl gegen
/// den Original-Text (inkl. `sudo`/`doas`-Präfix) als auch gegen den davon
/// befreiten Text geprüft. Abschnitt 4.6 verlangt, das Präfix "vor dem
/// Matching" zu entfernen (damit z. B. eine Allow-Regel "apt update" auch
/// `sudo apt update` erfasst, ohne dass man sie doppelt pflegen muss),
/// während Testfall 7 eine Regel "sudo -> Confirm" ermöglichen soll — mit den
/// Rule-/Pattern-Typen aus Abschnitt 2 gibt es dafür kein eigenes
/// Matching-Feld (kein `elevated`-Flag am Pattern). Dual-Matching gegen
/// beide Text-Varianten löst den Widerspruch, ohne die in Abschnitt 2 fest
/// vorgegebenen Typen zu erweitern. Siehe `docs/adr/0002-sudo-dual-text-matching.md`.
/// Wie zuvor `evaluate_rules`, liefert zusätzlich die `RuleId` der
/// gegriffenen Regel (falls eine gegriffen hat) für `EvaluationTrace`.
fn evaluate_rules_explained(
    rules: &[Rule],
    original: &str,
    stripped: &str,
) -> (Decision, Option<RuleId>) {
    for action in [RuleAction::Deny, RuleAction::Confirm, RuleAction::Allow] {
        let mut bucket: Vec<&Rule> = rules.iter().filter(|r| r.action == action).collect();
        bucket.sort_by(|a, b| {
            scope_rank(&b.scope)
                .cmp(&scope_rank(&a.scope))
                .then_with(|| b.priority.cmp(&a.priority))
        });

        for rule in bucket {
            let is_match = rule.pattern.matches(original)
                || (original != stripped && rule.pattern.matches(stripped));
            if is_match {
                let decision = match action {
                    RuleAction::Deny => Decision::Deny {
                        reason: format!("Regel '{}' (Deny)", rule.id),
                    },
                    RuleAction::Confirm => Decision::Confirm {
                        reason: format!("Regel '{}' (Confirm)", rule.id),
                    },
                    RuleAction::Allow => Decision::AutoExec,
                };
                return (decision, Some(rule.id.clone()));
            }
        }
    }
    (
        Decision::Confirm {
            reason: "keine Regel gefunden".to_string(),
        },
        None,
    )
}

/// Ob `scope` für eine Auswertung im gegebenen `effective` Scope gilt (Spec
/// 0002, Abschnitt 2/5): `Global` gilt immer, `Server`/`Tag` nur bei exakter
/// Übereinstimmung. Öffentlich, damit `PolicyStore`-Implementierungen (z. B.
/// `SqlitePolicyStore` in `persistence-sqlite`) dieselbe Logik verwenden wie
/// die In-Memory-Testimplementierungen in diesem Crate, statt sie an jeder
/// Implementierungsstelle erneut nachzubauen.
pub fn scope_applies(scope: &Scope, effective: &EffectiveScope) -> bool {
    match scope {
        Scope::Global => true,
        Scope::Tag(tag) => effective.tags.contains(tag),
        Scope::Server(id) => *id == effective.server_id,
    }
}

fn scope_rank(scope: &Scope) -> u8 {
    match scope {
        Scope::Server(_) => 2,
        Scope::Tag(_) => 1,
        Scope::Global => 0,
    }
}

fn severity(decision: &Decision) -> u8 {
    match decision {
        Decision::AutoExec => 0,
        Decision::Confirm { .. } => 1,
        Decision::Deny { .. } => 2,
    }
}

/// Kombiniert zwei Decisions zum strengeren Ergebnis (`Deny > Confirm >
/// AutoExec`, Spec Abschnitt 3/4.3). `AutoExec` ist das neutrale Element
/// (kombiniert mit irgendetwas ergibt es immer das andere Ergebnis), daher
/// als Startwert für Falten über mehrere Teilkommandos geeignet.
fn combine(a: Decision, b: Decision) -> Decision {
    match severity(&a).cmp(&severity(&b)) {
        Ordering::Greater => a,
        Ordering::Less => b,
        Ordering::Equal => match (a, b) {
            (Decision::AutoExec, Decision::AutoExec) => Decision::AutoExec,
            (Decision::Confirm { reason: r1 }, Decision::Confirm { reason: r2 }) => {
                Decision::Confirm {
                    reason: merge_reasons(r1, r2),
                }
            }
            (Decision::Deny { reason: r1 }, Decision::Deny { reason: r2 }) => Decision::Deny {
                reason: merge_reasons(r1, r2),
            },
            _ => unreachable!("gleiche severity impliziert gleiche Decision-Variante"),
        },
    }
}

/// Fügt zwei (ggf. bereits selbst gemergte) Reason-Strings zusammen, ohne
/// einzelne Bestandteile doppelt aufzuführen. Ein Kommando mit mehreren
/// `&&`-Teilkommandos und/oder einer `$(...)`-Substitution erzeugt sonst
/// schnell einen langen, redundanten String wie "keine Regel gefunden;
/// keine Regel gefunden; Command-Substitution ... erkannt; keine Regel
/// gefunden" — ein reiner `a == b`-Vergleich der GESAMTEN Strings dedupliziert
/// nur den Fall, dass beide Seiten identisch sind, nicht wiederkehrende
/// Einzelteile innerhalb eines bereits zusammengesetzten Strings.
fn merge_reasons(a: String, b: String) -> String {
    let mut parts: Vec<&str> = a.split("; ").collect();
    for part in b.split("; ") {
        if !parts.contains(&part) {
            parts.push(part);
        }
    }
    parts.join("; ")
}
