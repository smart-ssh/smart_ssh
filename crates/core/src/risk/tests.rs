//! Ein Test je Beispielmuster aus Spec 0026, Abschnitt 2, plus
//! Segmentierungs-/Eskalations-/SFTP-Mapping-Verhalten (Aufgabenstellung
//! Teil 1, Punkt 6).

use super::classifier::RuleBasedRiskClassifier;
use super::types::{RiskClassifier, RiskLevel};

fn classify(command: &str) -> super::types::RiskAssessment {
    RuleBasedRiskClassifier.classify(command)
}

// --- Server-Risiko, Rot ------------------------------------------------

#[test]
fn test_server_risk_red_rm_rf() {
    let a = classify("rm -rf /var/log/old");
    assert_eq!(a.server_risk, RiskLevel::Red);
}

#[test]
fn test_server_risk_red_dd_to_device() {
    let a = classify("dd if=/dev/zero of=/dev/sda");
    assert_eq!(a.server_risk, RiskLevel::Red);
}

#[test]
fn test_server_risk_red_mkfs() {
    let a = classify("mkfs.ext4 /dev/sdb1");
    assert_eq!(a.server_risk, RiskLevel::Red);
}

#[test]
fn test_server_risk_red_fork_bomb() {
    let a = classify(":(){ :|:& };:");
    assert_eq!(a.server_risk, RiskLevel::Red);
}

#[test]
fn test_server_risk_red_shutdown_reboot_poweroff() {
    assert_eq!(classify("shutdown -h now").server_risk, RiskLevel::Red);
    assert_eq!(classify("reboot").server_risk, RiskLevel::Red);
    assert_eq!(classify("poweroff").server_risk, RiskLevel::Red);
}

#[test]
fn test_server_risk_red_iptables_flush() {
    let a = classify("iptables -F");
    assert_eq!(a.server_risk, RiskLevel::Red);
}

#[test]
fn test_server_risk_red_chmod_recursive_777_root() {
    let a = classify("chmod -R 777 /");
    assert_eq!(a.server_risk, RiskLevel::Red);
}

// --- Server-Risiko, Gelb -------------------------------------------------

#[test]
fn test_server_risk_yellow_rm_without_rf() {
    let a = classify("rm old_file.txt");
    assert_eq!(a.server_risk, RiskLevel::Yellow);
}

#[test]
fn test_server_risk_yellow_systemctl_stop_restart() {
    assert_eq!(
        classify("systemctl stop nginx").server_risk,
        RiskLevel::Yellow
    );
    assert_eq!(
        classify("systemctl restart nginx").server_risk,
        RiskLevel::Yellow
    );
}

#[test]
fn test_server_risk_yellow_apt_yum_remove() {
    assert_eq!(classify("apt remove nginx").server_risk, RiskLevel::Yellow);
    assert_eq!(classify("yum remove nginx").server_risk, RiskLevel::Yellow);
}

#[test]
fn test_server_risk_yellow_git_reset_hard() {
    let a = classify("git reset --hard HEAD~3");
    assert_eq!(a.server_risk, RiskLevel::Yellow);
}

#[test]
fn test_server_risk_yellow_kill() {
    let a = classify("kill 4821");
    assert_eq!(a.server_risk, RiskLevel::Yellow);
}

// --- Daten-Risiko, Rot -----------------------------------------------

#[test]
fn test_data_risk_red_cat_id_rsa() {
    let a = classify("cat ~/.ssh/id_rsa");
    assert_eq!(a.data_risk, RiskLevel::Red);
}

#[test]
fn test_data_risk_red_less_head_tail_on_secret_files() {
    assert_eq!(classify("less /app/.env").data_risk, RiskLevel::Red);
    assert_eq!(classify("head server.key").data_risk, RiskLevel::Red);
    assert_eq!(classify("tail /etc/shadow").data_risk, RiskLevel::Red);
}

#[test]
fn test_data_risk_red_pem_credentials_aws() {
    assert_eq!(classify("cat cert.pem").data_risk, RiskLevel::Red);
    assert_eq!(classify("cat ./credentials.json").data_risk, RiskLevel::Red);
    assert_eq!(classify("cat ~/.aws/credentials").data_risk, RiskLevel::Red);
}

#[test]
fn test_data_risk_red_env_printenv() {
    assert_eq!(classify("env").data_risk, RiskLevel::Red);
    assert_eq!(classify("printenv").data_risk, RiskLevel::Red);
}

#[test]
fn test_data_risk_red_mysqldump_pg_dump() {
    assert_eq!(
        classify("mysqldump mydb > dump.sql").data_risk,
        RiskLevel::Red
    );
    assert_eq!(
        classify("pg_dump mydb > dump.sql").data_risk,
        RiskLevel::Red
    );
}

#[test]
fn test_data_risk_red_sql_select_user_password() {
    assert_eq!(
        classify("mysql -e 'SELECT * FROM users'").data_risk,
        RiskLevel::Red
    );
    assert_eq!(
        classify("mysql -e 'SELECT * FROM password_table'").data_risk,
        RiskLevel::Red
    );
}

// --- Daten-Risiko, Gelb ------------------------------------------------

#[test]
fn test_data_risk_yellow_find_key_files() {
    let a = classify("find / -name *.key");
    assert_eq!(a.data_risk, RiskLevel::Yellow);
}

#[test]
fn test_data_risk_yellow_ls_ssh_or_etc() {
    assert_eq!(classify("ls ~/.ssh").data_risk, RiskLevel::Yellow);
    assert_eq!(classify("ls /etc").data_risk, RiskLevel::Yellow);
}

#[test]
fn test_data_risk_yellow_grep_password_secret_token() {
    assert_eq!(
        classify("grep -r password /app").data_risk,
        RiskLevel::Yellow
    );
    assert_eq!(classify("grep -r secret /app").data_risk, RiskLevel::Yellow);
    assert_eq!(classify("grep -r token /app").data_risk, RiskLevel::Yellow);
}

// --- Unauffällige Kommandos ---------------------------------------------

#[test]
fn test_unremarkable_command_yields_no_risk_on_either_axis() {
    let a = classify("ls -la /var/www");
    assert_eq!(a.server_risk, RiskLevel::None);
    assert_eq!(a.data_risk, RiskLevel::None);
    assert_eq!(a.server_risk_reason, None);
    assert_eq!(a.data_risk_reason, None);
}

#[test]
fn test_ai_reviewed_defaults_to_false() {
    assert!(!classify("ls -la").ai_reviewed);
}

// --- Segmentierung (identisch zur Filter-Engine) ------------------------

#[test]
fn test_segmentation_and_ampersand_chaining_classifies_each_part() {
    // Ein unauffälliger erster Teil, ein riskanter zweiter Teil — das
    // Gesamtergebnis muss den riskanten Teil auffangen (Spec 0026,
    // Abschnitt 2: "jedes Teilkommando wird einzeln klassifiziert").
    let a = classify("echo hi && rm -rf /tmp/build");
    assert_eq!(a.server_risk, RiskLevel::Red);
}

#[test]
fn test_segmentation_semicolon_chaining_classifies_each_part() {
    let a = classify("echo hi; cat ~/.ssh/id_rsa");
    assert_eq!(a.data_risk, RiskLevel::Red);
}

#[test]
fn test_segmentation_command_substitution_classifies_inner_command() {
    // Dieselbe Rekursion wie `filter::engine` (Spec 0002, Abschnitt 4.5) —
    // das äußere Kommando ist unauffällig, das innere $(...)  ist riskant.
    let a = classify("echo $(cat ~/.ssh/id_rsa)");
    assert_eq!(a.data_risk, RiskLevel::Red);
}

// --- Gesamtergebnis nimmt das höchste Level -----------------------------

#[test]
fn test_overall_result_takes_highest_level_across_multiple_segments() {
    // Erster Teil triggert Gelb (rm ohne -rf), zweiter Teil triggert Rot
    // (rm -rf) — das Gesamtergebnis muss Rot sein, nicht das des zuletzt
    // ausgewerteten Segments oder ein "erster Treffer gewinnt".
    let a = classify("rm old.txt && rm -rf /tmp/cache");
    assert_eq!(a.server_risk, RiskLevel::Red);
}

#[test]
fn test_single_segment_matching_both_axes_sets_both_independently() {
    // "cat" allein triggert kein Server-Risiko-Muster, "id_rsa" triggert
    // Daten-Risiko Rot — die beiden Achsen bleiben unabhängig voneinander.
    let a = classify("cat ~/.ssh/id_rsa");
    assert_eq!(a.server_risk, RiskLevel::None);
    assert_eq!(a.data_risk, RiskLevel::Red);
}

// --- SFTP-Pfad-Mapping (Spec 0020, Abschnitt 4.1 — dieselbe Konvention) -

#[test]
fn test_sftp_write_pseudo_command_on_id_rsa_path_yields_data_risk_red() {
    // Exakt die Pseudokommando-Form aus `app-tauri::orchestration::
    // sftp_write_pseudo_command` (Spec 0020, Abschnitt 4.1): "sftp-write
    // <pfad>". Dieses Modul kennt `AiAction`/SFTP nicht selbst (das bleibt
    // Aufgabe des Aufrufers, s. Moduldoc), aber der Klassifizierer muss auf
    // dieser Textform korrekt reagieren.
    let a = classify("sftp-write /home/user/.ssh/id_rsa");
    assert_eq!(a.data_risk, RiskLevel::Red);
}

#[test]
fn test_sftp_read_pseudo_command_on_shadow_path_yields_data_risk_red() {
    let a = classify("sftp-read /etc/shadow");
    assert_eq!(a.data_risk, RiskLevel::Red);
}

#[test]
fn test_sftp_pseudo_command_on_unremarkable_path_yields_no_data_risk() {
    let a = classify("sftp-read /var/www/index.html");
    assert_eq!(a.data_risk, RiskLevel::None);
}

// --- Unabhängiger Review-Pass, Spec 0026 --------------------------------

/// Ohne Längenschranke rekursiert `segment_command` unbegrenzt tief pro
/// `$(...)`-Verschachtelung und stürzt den Prozess per Stack-Overflow ab
/// (empirisch verifiziert). Ein Kommando über der Filter-Engine-eigenen
/// Längenschranke darf den Klassifizierer nicht zum Absturz bringen — ein
/// unklassifiziertes Ergebnis ist der akzeptable Fail-safe.
#[test]
fn test_classify_does_not_crash_on_oversized_deeply_nested_command() {
    let deeply_nested = "$(".repeat(20_000);
    let a = classify(&deeply_nested);
    assert_eq!(a.server_risk, RiskLevel::None);
    assert_eq!(a.data_risk, RiskLevel::None);
}

#[test]
fn test_classify_yields_no_risk_for_command_just_over_the_length_cap() {
    let over_cap = "rm -rf / ".repeat(1000);
    assert!(over_cap.len() > crate::filter::DEFAULT_MAX_COMMAND_LENGTH);
    let a = classify(&over_cap);
    assert_eq!(a.server_risk, RiskLevel::None);
}

/// Ein vorangestelltes `sudo` darf die Daten-Risiko-Erkennung nicht
/// aushebeln — `sudo cat /etc/shadow` ist die praktisch häufigere Form von
/// `cat /etc/shadow` und mindestens genauso riskant.
#[test]
fn test_data_risk_red_sudo_cat_shadow_is_not_defeated_by_sudo_prefix() {
    let a = classify("sudo cat /etc/shadow");
    assert_eq!(a.data_risk, RiskLevel::Red);
}

#[test]
fn test_server_risk_red_sudo_shutdown_is_not_defeated_by_sudo_prefix() {
    let a = classify("sudo shutdown -h now");
    assert_eq!(a.server_risk, RiskLevel::Red);
}

#[test]
fn test_server_risk_red_sudo_chmod_recursive_777_root_not_defeated_by_sudo_prefix() {
    let a = classify("sudo chmod -R 777 /");
    assert_eq!(a.server_risk, RiskLevel::Red);
}
