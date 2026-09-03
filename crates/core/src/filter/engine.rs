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
                    code: FILTER_EMPTY_COMMAND.to_string(),
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
                    code: FILTER_COMMAND_TOO_LONG.to_string(),
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
                    code: FILTER_EMPTY_COMMAND.to_string(),
                },
                matched_rule: None,
                matched_hard_blacklist_entry: None,
                sub_command_traces: Vec::new(),
            },
            ParseResult::Ambiguous { reason } => EvaluationTrace {
                decision: Decision::Confirm {
                    reason,
                    code: FILTER_PARSE_AMBIGUOUS.to_string(),
                },
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

        // Zusätzlich zum Original- und Sudo-befreiten Text (Dual-Text-
        // Matching, ADR 0002 — dort geht es um Nutzerregeln) wird für die
        // Hard-Blacklist noch eine dritte, aggressiver normalisierte Fassung
        // geprüft: wiederholt um `sudo`/Wrapper-Präfixe befreit und am
        // ersten Wort entquotet (`resolve_effective_command`). Sonst
        // umgehen einfache Verschleierungen wie `env rm -rf /`,
        // `sudo -u root rm -rf /` oder `"rm" -rf /` die Blacklist komplett,
        // obwohl Abschnitt 3.1 sie ausdrücklich "unabhängig von
        // Nutzerregeln" verlangt (unabhängiger Review-Pass, Spec 0002).
        let resolved_literal = parser::resolve_effective_command(&original_literal);
        let matched_hard_blacklist_entry = blacklist::matching_entry(&original_literal)
            .or_else(|| blacklist::matching_entry(&stripped_literal))
            .or_else(|| blacklist::matching_entry(&resolved_literal));
        let blacklist_decision = if matched_hard_blacklist_entry.is_some() {
            Decision::Confirm {
                reason: "Hard-Blacklist: potenziell gefährliches Kommando".to_string(),
                code: FILTER_HARD_BLACKLIST.to_string(),
            }
        } else {
            Decision::AutoExec
        };

        // Ausgabe-Umleitung (`>`, `>>`, `2>`, ...) erzwingt mindestens
        // Confirm, unabhängig von einer sonst passenden Allow-Regel — die
        // Engine kannte Umleitungsziele bislang gar nicht, wodurch z. B.
        // `Allow: ls *` auch `ls -la > /etc/passwd` unbestätigt durchließ
        // (unabhängiger Review-Pass, Spec 0002).
        let redirection_decision =
            if parser::contains_unquoted_output_redirection(&original_literal) {
                Decision::Confirm {
                    reason: "Ausgabe-Umleitung (>, >>, 2>, ...) erkannt".to_string(),
                    code: FILTER_OUTPUT_REDIRECTION.to_string(),
                }
            } else {
                Decision::AutoExec
            };

        let (rule_decision, matched_rule) = evaluate_rules_explained(
            rules,
            &original_literal,
            &stripped_literal,
            &resolved_literal,
        );

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
                    code: FILTER_COMMAND_SUBSTITUTION.to_string(),
                },
            )
        };

        let decision = combine(
            combine(
                combine(rule_decision, substitution_decision),
                blacklist_decision,
            ),
            redirection_decision,
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
///
/// Seit dem unabhängigen Review-Pass (Spec 0009/0013) zusätzlich gegen eine
/// dritte, aggressiver normalisierte Fassung geprüft (`resolved`, s.
/// `parser::resolve_effective_command`) — sonst hätten Nutzer-Regeln
/// (insbesondere Deny) dieselbe Wrapper-/Sudo-Flag-/Variablenzuweisungs-/
/// Quoting-Lücke wie zuvor die Hard-Blacklist: eine `Deny "docker *"`-Regel
/// muss auch `env docker rm -f prod` erfassen, nicht nur den wörtlichen
/// Aufruf. Gilt einheitlich für alle drei Aktionen (auch Allow), damit das
/// Matching-Verhalten pro Regel konsistent bleibt, unabhängig davon, ob sie
/// gerade blockiert oder erlaubt.
///
/// Wie zuvor `evaluate_rules`, liefert zusätzlich die `RuleId` der
/// gegriffenen Regel (falls eine gegriffen hat) für `EvaluationTrace`.
fn evaluate_rules_explained(
    rules: &[Rule],
    original: &str,
    stripped: &str,
    resolved: &str,
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
                || (original != stripped && rule.pattern.matches(stripped))
                || (original != resolved && stripped != resolved && rule.pattern.matches(resolved));
            if is_match {
                let decision = match action {
                    RuleAction::Deny => Decision::Deny {
                        reason: format!("Regel '{}' (Deny)", rule.id),
                        code: FILTER_RULE_DENY.to_string(),
                    },
                    RuleAction::Confirm => Decision::Confirm {
                        reason: format!("Regel '{}' (Confirm)", rule.id),
                        code: FILTER_RULE_CONFIRM.to_string(),
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
            code: FILTER_NO_RULE_MATCHED.to_string(),
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
            (
                Decision::Confirm {
                    reason: r1,
                    code: c1,
                },
                Decision::Confirm {
                    reason: r2,
                    code: c2,
                },
            ) => Decision::Confirm {
                reason: merge_reasons(r1, r2),
                code: merge_codes(c1, c2),
            },
            (
                Decision::Deny {
                    reason: r1,
                    code: c1,
                },
                Decision::Deny {
                    reason: r2,
                    code: c2,
                },
            ) => Decision::Deny {
                reason: merge_reasons(r1, r2),
                code: merge_codes(c1, c2),
            },
            _ => unreachable!("gleiche severity impliziert gleiche Decision-Variante"),
        },
    }
}

// Spec 0024, Abschnitt 5: stabile Codes je Grund-Art, fürs Frontend-Mapping
// auf Übersetzungs-Keys (s. `Decision`-Doc-Kommentar in `types.rs`).
const FILTER_EMPTY_COMMAND: &str = "FILTER_EMPTY_COMMAND";
const FILTER_COMMAND_TOO_LONG: &str = "FILTER_COMMAND_TOO_LONG";
const FILTER_PARSE_AMBIGUOUS: &str = "FILTER_PARSE_AMBIGUOUS";
const FILTER_HARD_BLACKLIST: &str = "FILTER_HARD_BLACKLIST";
const FILTER_OUTPUT_REDIRECTION: &str = "FILTER_OUTPUT_REDIRECTION";
const FILTER_COMMAND_SUBSTITUTION: &str = "FILTER_COMMAND_SUBSTITUTION";
const FILTER_RULE_DENY: &str = "FILTER_RULE_DENY";
const FILTER_RULE_CONFIRM: &str = "FILTER_RULE_CONFIRM";
const FILTER_NO_RULE_MATCHED: &str = "FILTER_NO_RULE_MATCHED";

/// Rangfolge für `merge_codes` — je kleiner, desto vorrangiger. Orientiert
/// sich an derselben Bucket-Präzedenz wie die Entscheidungsfindung selbst
/// (Hard-Blacklist vor Nutzerregeln vor struktureller Analyse vor Default),
/// s. `evaluate_rules_explained`-Doc-Kommentar.
fn code_priority(code: &str) -> u8 {
    match code {
        FILTER_HARD_BLACKLIST => 0,
        FILTER_RULE_DENY => 1,
        FILTER_RULE_CONFIRM => 2,
        FILTER_COMMAND_SUBSTITUTION => 3,
        FILTER_OUTPUT_REDIRECTION => 4,
        FILTER_COMMAND_TOO_LONG => 5,
        FILTER_PARSE_AMBIGUOUS => 6,
        FILTER_NO_RULE_MATCHED => 7,
        FILTER_EMPTY_COMMAND => 8,
        _ => u8::MAX,
    }
}

/// Wählt bei zusammengeführten Reasons (`merge_reasons`) den nach obiger
/// Präzedenz wichtigsten Einzel-Code aus — `reason` selbst verliert dabei
/// nichts (enthält weiterhin beide Texte), nur `code` kann pro Decision
/// jeweils nur einen Wert tragen.
fn merge_codes(a: String, b: String) -> String {
    if code_priority(&a) <= code_priority(&b) {
        a
    } else {
        b
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

#[cfg(test)]
mod code_tests {
    use super::*;

    /// Spec 0024, Abschnitt 5: Codes müssen stabil und eindeutig sein — kein
    /// Code darf für zwei unterschiedliche Grund-Arten doppelt vergeben sein.
    #[test]
    fn test_filter_decision_codes_are_unique() {
        let codes = [
            FILTER_EMPTY_COMMAND,
            FILTER_COMMAND_TOO_LONG,
            FILTER_PARSE_AMBIGUOUS,
            FILTER_HARD_BLACKLIST,
            FILTER_OUTPUT_REDIRECTION,
            FILTER_COMMAND_SUBSTITUTION,
            FILTER_RULE_DENY,
            FILTER_RULE_CONFIRM,
            FILTER_NO_RULE_MATCHED,
        ];
        let mut unique = codes.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(
            codes.len(),
            unique.len(),
            "doppelt vergebener Filter-Code: {codes:?}"
        );
    }

    #[test]
    fn test_merge_codes_prefers_higher_priority_code() {
        assert_eq!(
            merge_codes(
                FILTER_NO_RULE_MATCHED.to_string(),
                FILTER_HARD_BLACKLIST.to_string()
            ),
            FILTER_HARD_BLACKLIST,
        );
        assert_eq!(
            merge_codes(
                FILTER_HARD_BLACKLIST.to_string(),
                FILTER_NO_RULE_MATCHED.to_string()
            ),
            FILTER_HARD_BLACKLIST,
        );
        assert_eq!(
            merge_codes(
                FILTER_RULE_CONFIRM.to_string(),
                FILTER_RULE_CONFIRM.to_string()
            ),
            FILTER_RULE_CONFIRM,
        );
    }
}
