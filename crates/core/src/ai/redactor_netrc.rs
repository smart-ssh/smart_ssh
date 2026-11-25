//! `.netrc`-Muster des Redactors (eigene Datei, weil `redactor.rs` an seiner
//! Größengrenze steht).
//!
//! Maßstab sind die Parser, die eine `.netrc` wirklich lesen: curl
//! (`lib/netrc.c`), ftp (`ruserpass.c`) und Python (`netrc`). Gemeinsam ist
//! ihnen: Felder in beliebiger Reihenfolge, auch über Zeilen; `account` ist
//! laut netrc(5) ein zweites Passwort; Werte dürfen in Anführungszeichen
//! stehen (mit `\"`, Leerzeichen und Zeilenumbruch darin), `\` maskiert auch
//! ohne Quotes das Folgezeichen (ftp, Python). curl liest die Schlüsselwörter
//! ohne Rücksicht auf Groß-/Kleinschreibung, ftp kennt `passwd` und `,` als
//! Trenner, Python `user` und `#`-Kommentare, `git credential-netrc` dazu
//! `port`/`protocol`/`path`.
//!
//! Im Zweifel wird redigiert: ein Wert in Quotes ohne schließendes Quote
//! reicht bis zum nächsten `"` oder Textende.
//!
//! Grenzen, zugunsten von Fließtext: Wörter, die kein netrc-Feld sind,
//! beenden die Struktur (curl überspringt sie). Eine einzelne Zeile ohne
//! `machine` davor zählt nur, wenn sie ganz aus Feldern besteht.

/// Ein Wert, auch über Zeilen.
const TOKEN: &str = r#"(?:"(?:[^"\\]|\\[\s\S])*"?\S*|(?:\\[\s\S]|\S)+)"#;
/// Ein Wert innerhalb einer Zeile.
const LINE_TOKEN: &str = r#"(?:"(?:[^"\\\r\n]|\\[^\r\n])*"?\S*|(?:\\[^\r\n]|\S)+)"#;
/// Zwischen Schlüsselwort und Wert.
const SEP: &str = r"[\s,]+";
/// Zwischen zwei Feldern, auch über `#`-Kommentarzeilen hinweg.
const GAP: &str = r"(?:[\s,]+(?:#[^\r\n]*)?)+";
const LINE_SEP: &str = r"[ \t,]+";
/// Felder mit Wert, die kein Geheimnis sind.
const FIELD: &str = "(?:login|user|port|protocol|path)";
const SECRET: &str = "(?:password|passwd|account)";
const PASSWORD: &str = "(?:password|passwd)";
/// Was Werkzeuge einer Zeile voranstellen: `cat -n`, `grep -n`, `grep -H`
/// (Pfad mit `/` oder `netrc`), Diff-Zeichen.
const LINE_PREFIX: &str =
    r"(?:[ \t]*\d+(?:\t|[:-])|[^\s:]*(?:/|netrc)[^\s:]*?[:-](?:\d+[:-])?|[-+<>][ \t]?)?";

/// Ein Eintrag ab `machine <h>` oder `default` bis zu seinem letzten
/// Geheimnis; Einträge in Folge ergeben einen Treffer. Ersetzt wird der
/// ganze Treffer (wie bisher `machine h login u password p`): so bleibt kein
/// zweites Geheimnis desselben Eintrags (`account`, ein zweites `password`)
/// stehen, dessen Anker schon verbraucht wäre.
///
/// `default` mitten in einer Zeile nur mit einem folgenden Feld wie
/// `login`; am Zeilenanfang endet ein `default` ohne Feld davor nur mit
/// `password`/`passwd`. So bleiben "The default password is …" und "Default
/// account is locked" stehen. `macdef` beendet den Eintrag; der Makrotext
/// ist kein netrc-Feld.
pub(super) fn entry_pattern() -> String {
    // Ein Feld darf ohne Wert stehen (`account` direkt vor `password p`,
    // `password` vor dem nächsten `machine`): sonst nähme der Treffer das
    // folgende Schlüsselwort als Wert und ließe das Geheimnis dahinter stehen.
    let elem = format!("(?:(?:machine|{FIELD}|{SECRET})(?:{SEP}{TOKEN})?|default)");
    let chain = format!("(?:{elem}{GAP})*");
    let secret = format!("{SECRET}{SEP}{TOKEN}");
    let password = format!("{PASSWORD}{SEP}{TOKEN}");
    format!(
        r"(?i)(?:\bmachine{SEP}{TOKEN}{GAP}{chain}{secret}|\bdefault\s+{FIELD}{SEP}{TOKEN}{GAP}{chain}{secret}|(?m:^)[ \t]*default\s(?:{GAP})?(?:{chain}{password}(?:{GAP}{chain}{secret})?|(?:{elem}{GAP})+{secret}))"
    )
}

/// `password <wert>` als ganze Zeile, groß oder klein geschrieben. Ein Wert
/// mit `=`/`:` vorn gehört dem Schlüsselwort-Muster. `head` (das Präfix
/// eines Werkzeugs) bleibt stehen.
pub(super) fn password_line_pattern() -> String {
    format!(
        r#"(?imR)(?P<head>^{LINE_PREFIX})[ \t]*password[ \t]+(?:"(?:[^"\\]|\\[\s\S])*"?\S*|[^\s:=]\S*)[ \t]*$"#
    )
}

/// Eine Zeile eines mehrzeiligen Eintrags ohne `machine` davor (`grep`,
/// `tail`): die Zeile besteht nur aus netrc-Feldern, eines davon ein
/// Geheimnis. Nur klein geschrieben, wie netrc-Dateien üblicherweise aussehen:
/// "Login failed: password incorrect" ist eine Logzeile. `passwd` und
/// `account` brauchen ein weiteres Feld (`passwd root` ist ein Kommando),
/// `account` allein zählt nur eingerückt.
pub(super) fn field_line_pattern() -> String {
    let elem =
        format!("(?:(?:machine|login|user|account|password|passwd){LINE_SEP}{LINE_TOKEN}|default)");
    let value = format!("{LINE_SEP}{TOKEN}");
    let rest = format!("(?:{LINE_SEP}{elem})*");
    format!(
        r"(?mR)(?P<head>^{LINE_PREFIX})(?:[ \t]*(?:(?:{elem}{LINE_SEP})*password{value}{rest}|(?:{elem}{LINE_SEP})+(?:passwd|account){value}{rest}|(?:passwd|account){value}(?:{LINE_SEP}{elem})+)|[ \t]+account{value})(?:[ \t]+#[^\r\n]*)?[ \t]*$"
    )
}
