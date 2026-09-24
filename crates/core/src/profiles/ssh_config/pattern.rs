//! Host-Muster aus einer `ssh_config` (Spec 0075, §3.1.3).
//!
//! OpenSSH kennt in `Host`-Zeilen genau zwei Platzhalter — `*` für beliebig
//! viele Zeichen, `?` für genau eines — und das Präfix `!` für eine
//! Ausnahme. Bewusst **kein** `globset`/`regex`: Beide behandeln `/` oder
//! Zeichenklassen gesondert, und ein Muster aus einer fremden Datei soll
//! weder eine eigene Syntax mitbringen noch eine Laufzeit, die von seiner
//! Form abhängt (§5.6).

/// Eine einzelne Angabe in einer `Host`-Zeile, z. B. `web1`, `*.prod.de`
/// oder `!secret.prod.de`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostClause {
    /// Das Muster **ohne** ein führendes `!`, wörtlich wie in der Datei.
    pub pattern: String,
    pub negated: bool,
}

impl HostClause {
    /// Zerlegt ein Wort aus einer `Host`-Zeile. Ein einzelnes `!` ohne
    /// folgendes Muster ergibt ein leeres, negiertes Muster — das trifft
    /// dann auf nichts und ist damit wirkungslos, statt zu einem Profil
    /// namens `!` zu führen.
    pub fn parse(word: &str) -> Self {
        match word.strip_prefix('!') {
            Some(rest) => Self {
                pattern: rest.to_string(),
                negated: true,
            },
            None => Self {
                pattern: word.to_string(),
                negated: false,
            },
        }
    }

    /// Trägt dieses Muster einen Platzhalter? Ein negiertes Muster gilt
    /// nach §3.1.3 ebenfalls als Platzhalter, auch wenn sein Rumpf
    /// buchstäblich ist — `!secret` beschreibt keinen einzelnen Server.
    pub fn is_wildcard(&self) -> bool {
        self.negated || self.pattern.contains('*') || self.pattern.contains('?')
    }

    pub fn matches(&self, name: &str) -> bool {
        matches_pattern(&self.pattern, name)
    }
}

/// Gleicht `name` gegen ein OpenSSH-Host-Muster ab.
///
/// Zwei-Zeiger-Verfahren mit Rücksprungmarke: linear in
/// `pattern.len() * name.len()` im schlechtesten Fall und **ohne
/// Rekursion**. Ein rekursiver oder rückverfolgender Abgleich ließe sich
/// mit einem Muster wie `*a*a*a*a*b` aus einer fremden Datei in
/// exponentielle Laufzeit treiben; genau das schließt §5.6 aus.
pub fn matches_pattern(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();

    let mut pi = 0usize;
    let mut ni = 0usize;
    // Position des zuletzt gesehenen `*` im Muster und der zugehörige
    // Stand im Namen; `None` = bisher kein `*` passiert.
    let mut star: Option<usize> = None;
    let mut resume = 0usize;

    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            resume = ni;
            pi += 1;
        } else if let Some(s) = star {
            // Das letzte `*` einen Buchstaben mehr schlucken lassen.
            pi = s + 1;
            resume += 1;
            ni = resume;
        } else {
            return false;
        }
    }

    // Übrig gebliebene `*` dürfen leer bleiben, alles andere nicht.
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Trifft der Block mit diesen Angaben auf `name` zu?
///
/// OpenSSH-Regel: Es muss ein nicht-negiertes Muster passen **und** kein
/// negiertes. Die Ausnahme gewinnt also immer — sie kann einen Block nur
/// enger machen, nie weiter. Das ist die Richtung, die für uns
/// unbedenklich ist: Ein `!` kann dazu führen, dass eine Vorgabe **nicht**
/// greift, nie dazu, dass eine zusätzliche greift.
pub fn block_matches(clauses: &[HostClause], name: &str) -> bool {
    let positive = clauses
        .iter()
        .any(|c| !c.negated && matches_pattern(&c.pattern, name));
    if !positive {
        return false;
    }
    !clauses
        .iter()
        .any(|c| c.negated && matches_pattern(&c.pattern, name))
}
