//! Typed proof that an AI/MCP action went through the approval path.
//!
//! `ApprovedCommand` (and `ApprovedFileWrite` for the sudo fallback of a
//! confirmed file write) can only be created inside `orchestration`: on the
//! auto-run branch after `escalate()` returned `AutoExec`, and after the
//! user approved (including an edited command that passed the
//! re-evaluation). `execute_suggested_command` and `execute_privileged`
//! accept nothing else, so a command text cannot reach the SSH transport
//! from this path without an approval. The guard test
//! `exec_call_sites_guard` (in `tests_exec_guard.rs`) covers new call sites
//! elsewhere.

use ssh_manager_core::profiles::AiAction;

/// A command text that was shown to and approved by the user (or allowed
/// by a rule that resolved to auto-run).
///
/// The fields are private and there is no `Deserialize`, `Default` or
/// `From<String>`; the constructor is only visible inside `orchestration`.
///
/// A struct literal outside this module does not compile:
///
/// ```compile_fail
/// use app_logic::orchestration::ApprovedCommand;
/// let _ = ApprovedCommand { command: String::from("ls") };
/// ```
///
/// Neither does calling the constructor from outside the crate:
///
/// ```compile_fail
/// use app_logic::orchestration::ApprovedCommand;
/// let _ = ApprovedCommand::new(String::from("ls"));
/// ```
#[derive(Debug, Clone)]
pub struct ApprovedCommand {
    command: String,
}

impl ApprovedCommand {
    pub(super) fn new(command: String) -> Self {
        Self { command }
    }

    /// The approved command text.
    pub fn as_str(&self) -> &str {
        &self.command
    }
}

/// Proof that a `WriteRemoteFile` action was approved; required by the
/// privileged (sudo) write fallback.
#[derive(Debug, Clone)]
pub struct ApprovedFileWrite {
    path: String,
    content: String,
}

impl ApprovedFileWrite {
    pub(super) fn new(path: String, content: String) -> Self {
        Self { path, content }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn content(&self) -> &str {
        &self.content
    }
}

/// An action that may be executed. Built only by the two approval
/// constructors below.
pub(super) enum ApprovedAction {
    Command(ApprovedCommand),
    FileWrite(ApprovedFileWrite),
    /// Every other action type (notes, file read, ...), which does not
    /// reach the transport as a command.
    Other(AiAction),
}

impl ApprovedAction {
    /// Auto-run branch: call only after `escalate()` returned `AutoExec`.
    pub(super) fn from_auto_exec(action: AiAction) -> Self {
        Self::wrap(action)
    }

    /// The user approved the action (also an edited command that passed
    /// the re-evaluation).
    pub(super) fn from_user_approval(action: AiAction) -> Self {
        Self::wrap(action)
    }

    fn wrap(action: AiAction) -> Self {
        match action {
            AiAction::SuggestCommand { command } => Self::Command(ApprovedCommand::new(command)),
            AiAction::WriteRemoteFile { path, content } => {
                Self::FileWrite(ApprovedFileWrite::new(path, content))
            }
            other => Self::Other(other),
        }
    }
}
