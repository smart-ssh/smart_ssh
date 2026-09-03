use chrono::Utc;
use uuid::Uuid;

use super::store::{ProfileResult, ProfileStore};
use super::types::{NoteEditor, NoteRevision, NoteTarget, Server};

/// Baut den effektiven LLM-Kontext für eine Session zu `server` (Spec 0003,
/// Abschnitt 5.1): Gruppenkette von der Wurzel bis zur unmittelbaren Gruppe,
/// danach der Server selbst — vom Allgemeinen zum Spezifischen. Leere (oder
/// nur aus Whitespace bestehende) Notizen werden übersprungen, statt als
/// leerer `## Kontext: ...`-Abschnitt mitgerendert zu werden.
///
/// Abweichend vom Signatur-Vorschlag in der Spec (`-> String`) liefert diese
/// Funktion `ProfileResult<String>`: eine zyklische Gruppen-Elternkette
/// (durch einen Store-Fehler, s. [`ProfileStore::group_chain`]) muss einen
/// Fehler ergeben können, nicht endlos laufen oder panicken — das lässt sich
/// mit einem reinen `-> String`-Rückgabetyp nicht ausdrücken.
///
/// Design-Entscheidung zu einem offenen Punkt aus Spec Abschnitt 6: es gibt
/// **keine** Kürzung/Priorisierung nach Länge oder Token-Budget — der volle
/// zusammengesetzte Text wird zurückgegeben, egal wie tief die
/// Gruppenhierarchie oder wie lang die einzelnen Notizen sind. Begründung:
/// ein sinnvolles Limit hängt vom tatsächlich verwendeten KI-Modell ab
/// (Kontextfenster, Preis pro Token) — das ist Sache der noch nicht
/// existierenden KI-Provider-Spec, die den Aufrufer dieser Funktion bilden
/// wird. Würde hier vorab künstlich gekürzt, müsste die Provider-Spec diese
/// Kürzung ggf. wieder umgehen oder doppelt Buchhaltung führen. Siehe
/// `docs/adr/0004-effective-notes-kein-truncation-mvp.md`.
///
/// `async`, weil [`ProfileStore::group_chain`] seit der Umstellung auf
/// `async-trait` (Teil 1 der SQLite-Persistenz-Anbindung, Spec 0004)
/// selbst `async` ist — diese Funktion ruft es auf und muss das Ergebnis
/// awaiten, kann also nicht mehr synchron bleiben.
pub async fn effective_notes(server: &Server, store: &dyn ProfileStore) -> ProfileResult<String> {
    let sections = effective_notes_sections(server, store).await?;
    Ok(sections
        .into_iter()
        .map(|(label, notes)| format!("## Kontext: {label}\n{notes}"))
        .collect::<Vec<_>>()
        .join("\n\n"))
}

/// Wie [`effective_notes`], liefert die einzelnen Abschnitte aber
/// unformatiert als (Quelle, Notiztext)-Paare statt als einen
/// zusammengefügten String — Grundlage für den KI-Kontext-Aufbau (Spec
/// 0039): dort muss jeder Abschnitt einzeln über `ai::fence_untrusted`
/// laufen, bevor er in den System-Prompt eingebettet wird, was mit einem
/// bereits zusammengefügten String nicht mehr möglich wäre. `effective_notes`
/// bleibt für die menschliche Vorschau (`preview_effective_notes`) und den
/// MCP-Notizen-Zugriff unverändert bestehen — beide wollen lesbaren Text,
/// kein Fencing.
pub async fn effective_notes_sections(
    server: &Server,
    store: &dyn ProfileStore,
) -> ProfileResult<Vec<(String, String)>> {
    let mut sections = Vec::new();

    if let Some(group_id) = server.group_id {
        for group in store.group_chain(&group_id).await? {
            push_section(&mut sections, group.name, group.notes);
        }
    }

    push_section(
        &mut sections,
        format!("Server \"{}\"", server.name),
        server.notes.clone(),
    );

    Ok(sections)
}

fn push_section(sections: &mut Vec<(String, String)>, label: String, notes: String) {
    if notes.trim().is_empty() {
        return;
    }
    sections.push((label, notes));
}

/// Erzeugt einen neuen [`NoteRevision`]-Eintrag mit Zeitstempel (Spec 0003,
/// Abschnitt 5.3). Persistiert nichts selbst — das ist Aufgabe der
/// (künftigen) Store-Anbindung, hier entsteht nur die Struktur.
pub fn record_revision(target: NoteTarget, content: String, editor: NoteEditor) -> NoteRevision {
    NoteRevision {
        id: Uuid::new_v4(),
        target,
        content,
        edited_by: editor,
        created_at: Utc::now(),
    }
}
