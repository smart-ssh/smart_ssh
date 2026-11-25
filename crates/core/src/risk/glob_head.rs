/// Concrete strings a glob head (the part before a literal `id_`) can stand
/// for: `*` is nothing or `b`, `?` is `b`, `[..]` its members, `{a,b}` its
/// alternatives. Capped; an unparseable head yields no candidates beyond a
/// plain wildcard-free reading.
pub(super) fn expand_glob_head(head: &str) -> Vec<String> {
    const CAP: usize = 64;
    let chars: Vec<char> = head.chars().collect();
    let mut out: Vec<String> = vec![String::new()];
    let mut i = 0;
    while i < chars.len() {
        let options: Vec<String> = match chars[i] {
            '*' => vec![String::new(), "b".into()],
            '?' => vec!["b".into()],
            '[' => {
                let end = chars[i..].iter().position(|&c| c == ']').map(|p| i + p);
                let Some(end) = end.filter(|&e| e > i + 1) else {
                    return Vec::new();
                };
                let body = &chars[i + 1..end];
                i = end;
                if matches!(body.first(), Some('!' | '^')) {
                    vec!["b".into()]
                } else {
                    body.iter()
                        .filter(|c| **c != '-')
                        .map(|c| c.to_string())
                        .collect()
                }
            }
            '{' => {
                let end = chars[i..].iter().position(|&c| c == '}').map(|p| i + p);
                let Some(end) = end else {
                    return Vec::new();
                };
                let body: String = chars[i + 1..end].iter().collect();
                i = end;
                if body.contains(['{', '*', '?', '[']) {
                    return Vec::new();
                }
                body.split(',').map(str::to_string).collect()
            }
            c => vec![c.to_string()],
        };
        i += 1;
        out = out
            .iter()
            .flat_map(|prefix| options.iter().map(move |o| format!("{prefix}{o}")))
            .take(CAP)
            .collect();
    }
    out
}
