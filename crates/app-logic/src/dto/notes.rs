//! DTOs: notes.

use serde::Serialize;

use ssh_manager_core::profiles::{NoteEditor, NoteRevision};

/// Wer eine [`NoteRevision`] erzeugt hat, für die Anzeige in der
/// Notiz-Historie (Spec 0008, Abschnitt 6: "Editor (Nutzer, oder KI inkl.
/// Provider/Modell-Name)").
#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum NoteEditorDto {
    User,
    Ai { provider: String, model: String },
}

impl From<&NoteEditor> for NoteEditorDto {
    fn from(editor: &NoteEditor) -> Self {
        match editor {
            NoteEditor::User => NoteEditorDto::User,
            NoteEditor::Ai { provider, model } => NoteEditorDto::Ai {
                provider: provider.clone(),
                model: model.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteRevisionDto {
    pub id: String,
    pub content: String,
    pub edited_by: NoteEditorDto,
    pub created_at: String,
}

impl From<&NoteRevision> for NoteRevisionDto {
    fn from(revision: &NoteRevision) -> Self {
        Self {
            id: revision.id.to_string(),
            content: revision.content.clone(),
            edited_by: NoteEditorDto::from(&revision.edited_by),
            created_at: revision.created_at.to_rfc3339(),
        }
    }
}
