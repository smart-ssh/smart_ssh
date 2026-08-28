use std::cmp::Ordering;

use super::blacklist;
use super::parser::{self, ParseResult};
use super::types::{Decision, EffectiveScope, EvalContext, Rule, RuleAction, Scope};

/// Ab dieser Zeichenlänge wird ein Kommando ungeprüft auf `Confirm` gesetzt,
/// statt es vollständig zu parsen (Spec 0002, Abschnitt 6, Testfall 11).
/// Über [`FilterEngine::with_max_command_length`] konfigurierbar.
pub const DEFAULT_MAX_COMMAND_LENGTH: usize = 4096;

/// Quelle für die auf einen [`EffectiveScope`] anwendbaren [`Rule`]s (Spec
/// 0002, Abschnitt 5).
///
/// Als Trait modelliert, damit Tests eine In-Memory-Implementierung nutzen
/// können, ohne von der späteren Datenbank-Anbindung abzuhängen.
pub trait PolicyStore {
    fn rules_for(&self, scope: &EffectiveScope) -> Vec<Rule>;
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
    pub fn evaluate(&self, command: &str, ctx: &EvalContext) -> Decision {
        if command.trim().is_empty() {
            // Testfall 10: leerer/reiner Whitespace-String -> Deny, nicht nur
            // Confirm, da es schlicht kein Kommando gibt, das ausgeführt
            // werden könnte.
            return Decision::Deny {
                reason: "kein sinnvolles Kommando (leere Eingabe)".to_string(),
            };
        }
        if command.chars().count() > self.max_command_length {
            // Testfall 11: bewusst VOR jedem Parsing/Blacklist-Scan geprüft,
            // damit extrem lange Payloads gar nicht erst vollständig
            // analysiert werden müssen.
            return Decision::Confirm {
                reason: format!(
                    "Kommando überschreitet Längenlimit von {} Zeichen",
                    self.max_command_length
                ),
            };
        }

        let scope = EffectiveScope::from(ctx);
        let rules = self.store.rules_for(&scope);
        self.evaluate_parsed(command, &rules)
    }

    fn evaluate_parsed(&self, command: &str, rules: &[Rule]) -> Decision {
        match parser::split_command(command) {
            ParseResult::Empty => Decision::Deny {
                reason: "kein sinnvolles Kommando (leere Eingabe)".to_string(),
            },
            ParseResult::Ambiguous { reason } => Decision::Confirm { reason },
            ParseResult::Segments(segments) => segments
                .into_iter()
                .map(|segment| self.evaluate_segment(&segment, rules))
                .fold(Decision::AutoExec, combine),
        }
    }

    fn evaluate_segment(&self, raw_segment: &str, rules: &[Rule]) -> Decision {
        let normalized = parser::normalize_whitespace(raw_segment);
        // `elevated` wird bewusst nicht in die öffentliche `Decision`
        // geschrieben (die laut Spec Abschnitt 5 exakt AutoExec/Confirm/Deny
        // ist) — siehe Kommentar bei `text_matches` dazu, wie das Elevated-
        // Wissen stattdessen ins Matching einfließt.
        let (_elevated, stripped) = parser::detect_elevation(&normalized);

        let (original_literal, _) = parser::strip_substitutions(&normalized);
        let (stripped_literal, inner_contents) = parser::strip_substitutions(&stripped);
        let original_literal = parser::normalize_whitespace(&original_literal);
        let stripped_literal = parser::normalize_whitespace(&stripped_literal);

        let blacklist_decision = if blacklist::matches_any(&original_literal)
            || blacklist::matches_any(&stripped_literal)
        {
            Decision::Confirm {
                reason: "Hard-Blacklist: potenziell gefährliches Kommando".to_string(),
            }
        } else {
            Decision::AutoExec
        };

        let rule_decision = evaluate_rules(rules, &original_literal, &stripped_literal);

        let substitution_decision = if inner_contents.is_empty() {
            Decision::AutoExec
        } else {
            let inner_decision = inner_contents
                .into_iter()
                .map(|inner| self.evaluate_parsed(&inner, rules))
                .fold(Decision::AutoExec, combine);
            combine(
                inner_decision,
                Decision::Confirm {
                    reason: "Command-Substitution ($(...) / `...`) erkannt".to_string(),
                },
            )
        };

        combine(
            combine(rule_decision, substitution_decision),
            blacklist_decision,
        )
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
/// vorgegebenen Typen zu erweitern. Siehe ADR-Vorschlag in der
/// Abschluss-Nachricht.
fn evaluate_rules(rules: &[Rule], original: &str, stripped: &str) -> Decision {
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
                return match action {
                    RuleAction::Deny => Decision::Deny {
                        reason: format!("Regel '{}' (Deny)", rule.id),
                    },
                    RuleAction::Confirm => Decision::Confirm {
                        reason: format!("Regel '{}' (Confirm)", rule.id),
                    },
                    RuleAction::Allow => Decision::AutoExec,
                };
            }
        }
    }
    Decision::Confirm {
        reason: "keine Regel gefunden".to_string(),
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

fn merge_reasons(a: String, b: String) -> String {
    if a == b {
        a
    } else {
        format!("{a}; {b}")
    }
}
