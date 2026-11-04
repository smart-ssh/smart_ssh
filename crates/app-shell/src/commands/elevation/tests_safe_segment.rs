//! Spec-Reviewer-Fund (Spec 0054, Review des Gesamtpakets, ERHÖHTE
//! Priorität): Zip-Slip über servergelieferte Dateinamen beim
//! rekursiven Ordner-Download/"Lokal öffnen". `download_recursive`/
//! `download_entry_to`/`sftp_open_for_editing` rufen `safe_local_segment`
//! jetzt vor jedem `PathBuf::join(entry.name)` auf — diese Tests prüfen
//! nur die reine Funktion (`std::path::Path::join`s Verhalten bei
//! `..`-Segmenten/absoluten Pfaden ist dokumentiertes, stabiles
//! Standardbibliotheks-Verhalten, kein separat zu beweisender Teil).
use super::*;

#[test]
fn test_accepts_a_plain_file_name() {
    assert!(safe_local_segment("readme.md").is_ok());
    assert!(safe_local_segment("nginx.conf.smartssh-backup-123").is_ok());
}

#[test]
fn test_rejects_parent_directory_traversal() {
    assert!(safe_local_segment("..").is_err());
    assert!(safe_local_segment("../etc/passwd").is_err());
    assert!(safe_local_segment("../../.zshrc").is_err());
}

#[test]
fn test_rejects_current_directory_segment() {
    assert!(safe_local_segment(".").is_err());
}

#[test]
fn test_rejects_an_embedded_separator() {
    assert!(safe_local_segment("a/b").is_err());
}

#[test]
fn test_rejects_an_absolute_path() {
    assert!(safe_local_segment("/etc/passwd").is_err());
    assert!(safe_local_segment("/Users/u/.ssh/authorized_keys").is_err());
}
