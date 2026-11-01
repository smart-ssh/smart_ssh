use ssh_manager_core::profiles::{AiAction, NoteTargetSelector};

use super::{sftp_read_pseudo_command, sftp_write_pseudo_command};

/// Command-like text shown in the history for a rejected action.
pub(super) fn describe_rejected_action(action: &AiAction) -> String {
    match action {
        AiAction::SuggestCommand { command } => command.clone(),
        AiAction::ReadRemoteFile { path } => sftp_read_pseudo_command(path),
        AiAction::WriteRemoteFile { path, .. } => sftp_write_pseudo_command(path),
        AiAction::ProposeNoteUpdate { target, .. } => {
            let target_label = match target {
                NoteTargetSelector::CurrentServer => "aktueller Server",
                NoteTargetSelector::CurrentServerGroup => "aktuelle Servergruppe",
            };
            format!("update-note ({target_label})")
        }
        AiAction::GenerateDocument { .. } => unreachable!(
            "GenerateDocument durchläuft nie evaluate_action/einen Bestätigungsdialog, s. dort"
        ),
        AiAction::AskUser { .. } => unreachable!(
            "AskUser is intercepted in run_one_round and never reaches a confirmation dialog"
        ),
    }
}
