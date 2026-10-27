//! Tests for `dto` (SFTP server path normalization).

use super::normalize_sftp_server_path;

#[test]
fn test_empty_override_means_automatic() {
    assert_eq!(normalize_sftp_server_path(None), Ok(None));
    assert_eq!(
        normalize_sftp_server_path(Some("   ".to_string())),
        Ok(None)
    );
}

#[test]
fn test_valid_override_is_trimmed_and_kept() {
    assert_eq!(
        normalize_sftp_server_path(Some(" /usr/lib/openssh/sftp-server ".to_string())),
        Ok(Some("/usr/lib/openssh/sftp-server".to_string()))
    );
}

#[test]
fn test_unsafe_override_is_rejected() {
    for bad in [
        "/bin/sh",
        "sftp-server",
        "/usr/lib/x; reboot",
        "/usr/lib/$(id)",
        "/a/../b",
    ] {
        assert!(
            normalize_sftp_server_path(Some(bad.to_string())).is_err(),
            "{bad}"
        );
    }
}

// --- Spec 0073, A6–A8 (Q-BL-0149-01) -----------------------------------
//
// Der Pfad-Override läuft über denselben geteilten
// `trim_credential_value` wie die Zugangsdaten-Werte. Die
// Sicherheitsprüfung `is_plausible_sftp_server_path` ist dabei wörtlich
// unverändert geblieben und läuft weiterhin auf dem Endwert — die drei
// Tests halten beides fest: Was am **Rand** klebt, wird bereinigt; was
// **innen** steht, wird weiterhin abgelehnt.

#[test]
fn test_invisible_edge_chars_are_cleaned_not_rejected() {
    // Gegen den Stand vor der Umstellung rot: `str::trim` lässt BOM und
    // Zero-Width-Space stehen, der Pfad fiel danach durch
    // `is_plausible_sftp_server_path` und endete als Fehler.
    assert_eq!(
        normalize_sftp_server_path(Some(
            "\u{FEFF} /usr/lib/openssh/sftp-server \u{200B}".to_string()
        )),
        Ok(Some("/usr/lib/openssh/sftp-server".to_string()))
    );
}

#[test]
fn test_invisible_char_inside_is_still_rejected() {
    // Der Kern der Entscheidung: Bereinigt wird nur der Rand. Ein
    // unsichtbares Zeichen **innerhalb** des Pfades überlebt das Trimmen
    // und wird von der unveränderten Prüfung abgelehnt — es kann also in
    // kein `sudo`-Kommando gelangen.
    for bad in [
        "/usr/lib/open\u{200B}ssh/sftp-server",
        "/usr/lib/openssh/sftp-\u{FEFF}server",
        "/usr/lib\u{2060}/openssh/sftp-server",
    ] {
        assert!(
            normalize_sftp_server_path(Some(bad.to_string())).is_err(),
            "{bad}"
        );
    }
}

#[test]
fn test_override_of_only_invisible_chars_means_automatic() {
    // Wie „nur Leerzeichen": nach dem Trimmen leer, und leer heißt
    // „automatisch" (`None`), nicht „ungültiger Pfad".
    assert_eq!(
        normalize_sftp_server_path(Some("\u{FEFF}\u{200B}\u{2060}".to_string())),
        Ok(None)
    );
    assert_eq!(
        normalize_sftp_server_path(Some(" \u{FEFF} \u{200C}\u{200D}\t\r\n".to_string())),
        Ok(None)
    );
}
