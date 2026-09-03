//! Markdown→DOCX-Konvertierung für KI-generierte Dokumente (Spec 0012,
//! Abschnitt 4). Bewusst reine, IO-freie Funktionen — der native
//! Speichern-Dialog und das eigentliche Schreiben auf die Festplatte
//! passieren erst im `export_document`-Command (`commands.rs`), damit diese
//! Konvertierung ohne Tauri-Laufzeit getestet werden kann.
//!
//! Konvertierungs-Entscheidung (Spec 0012, Abschnitt 4 nennt `docx-rs` nur
//! als Beispiel-Crate): tatsächlich verwendet wird die Crate `docx-rs`
//! (crates.io, `bokuweb/docx-rs`) — ihre tatsächliche API weicht in Details
//! von der Spec-Skizze ab (kein einzelner `save()`-Aufruf, sondern
//! `Docx::build().pack(writer)` gegen einen `Write + Seek`-Ziel; Styles wie
//! Überschriften müssen selbst mit Formatierung registriert werden, es gibt
//! keine impliziten "eingebauten" Word-Stile). Für Markdown selbst wird
//! `pulldown-cmark` verwendet (Event-Stream statt eigenem Parser). Beide
//! Entscheidungen sind unten an den jeweiligen Stellen kommentiert; s. auch
//! die vorgeschlagene ADR am Ende der Umsetzung.

use docx_rs::{
    AbstractNumbering, BreakType, Docx, IndentLevel, Level, LevelJc, LevelText, LineSpacing,
    NumberFormat, Numbering, NumberingId, Paragraph, Run, RunFonts, Start, Style, StyleType,
};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::dto::DocumentFormat;

const BULLET_NUMBERING_ID: usize = 1;
const ORDERED_NUMBERING_ID: usize = 2;
/// Maximale Verschachtelungstiefe, für die Listen einen eigenen (stärker
/// eingerückten) Nummerierungs-Level bekommen — tiefer verschachtelte Listen
/// werden auf diesen letzten Level abgeflacht (Spec 0012, Abschnitt 4:
/// "verschachtelte Listen ... vereinfacht dargestellt").
const LIST_INDENT_LEVELS: usize = 3;

/// Leitet aus dem KI-gelieferten Dokumenttitel und dem Zielformat einen
/// vorbelegten Dateinamen für den Speichern-Dialog ab (Spec 0012, Abschnitt
/// 3: "vorbelegt mit einem aus `title` abgeleiteten Dateinamen"). Entfernt
/// Zeichen, die in Dateinamen auf keinem der unterstützten Betriebssysteme
/// zuverlässig funktionieren; ein leerer/nur aus solchen Zeichen bestehender
/// Titel fällt auf "Dokument" zurück statt einen leeren Dateinamen zu
/// erzeugen.
pub fn default_export_file_name(title: &str, format: DocumentFormat) -> String {
    let extension = match format {
        DocumentFormat::Markdown => "md",
        DocumentFormat::Word => "docx",
    };
    let sanitized: String = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let base = if sanitized.is_empty() {
        "Dokument".to_string()
    } else {
        sanitized
    };
    format!("{base}.{extension}")
}

/// Wandelt Markdown-Inhalt (wie ihn `AiAction::GenerateDocument` liefert) in
/// die Bytes eines gültigen `.docx`-Dokuments um (Spec 0012, Abschnitt 4).
///
/// Moderne Typografie angelehnt an die UI-Darstellung im Chatfenster:
/// - Moderner serifenloser Font (`Segoe UI` / `Arial` / `Helvetica`)
/// - `Consolas` / `Courier New` für Code und Monospace
/// - Lesbare Absatz- und Zeilenabstände
pub fn markdown_to_docx_bytes(content_markdown: &str) -> Vec<u8> {
    let mut docx = Docx::new()
        .default_fonts(sans_fonts())
        .default_size(22)
        .default_line_spacing(LineSpacing::new().after(120).line(276))
        .add_style(heading_style("Heading1", "Heading 1", 32))
        .add_style(heading_style("Heading2", "Heading 2", 26))
        .add_style(heading_style("Heading3", "Heading 3", 22))
        .add_abstract_numbering(list_abstract_numbering(BULLET_NUMBERING_ID, "bullet", "•"))
        .add_numbering(Numbering::new(BULLET_NUMBERING_ID, BULLET_NUMBERING_ID))
        .add_abstract_numbering(list_abstract_numbering(
            ORDERED_NUMBERING_ID,
            "decimal",
            "%1.",
        ))
        .add_numbering(Numbering::new(ORDERED_NUMBERING_ID, ORDERED_NUMBERING_ID));

    for paragraph in parse_markdown_to_paragraphs(content_markdown) {
        docx = docx.add_paragraph(paragraph);
    }

    let mut buffer = std::io::Cursor::new(Vec::new());
    docx.build()
        .pack(&mut buffer)
        .expect("DOCX-Erzeugung im Arbeitsspeicher darf nicht fehlschlagen");
    buffer.into_inner()
}

fn heading_style(style_id: &str, display_name: &str, half_point_size: usize) -> Style {
    Style::new(style_id, StyleType::Paragraph)
        .name(display_name)
        .bold()
        .size(half_point_size)
        .fonts(sans_fonts())
}

fn heading_style_id(level: HeadingLevel) -> &'static str {
    match level {
        HeadingLevel::H1 => "Heading1",
        HeadingLevel::H2 => "Heading2",
        HeadingLevel::H3 | HeadingLevel::H4 | HeadingLevel::H5 | HeadingLevel::H6 => "Heading3",
    }
}

fn list_abstract_numbering(id: usize, format: &str, level_text: &str) -> AbstractNumbering {
    let mut numbering = AbstractNumbering::new(id);
    for level in 0..LIST_INDENT_LEVELS {
        numbering = numbering.add_level(
            Level::new(
                level,
                Start::new(1),
                NumberFormat::new(format),
                LevelText::new(level_text),
                LevelJc::new("left"),
            )
            .indent(Some(720 * (level as i32 + 1)), None, None, None),
        );
    }
    numbering
}

fn sans_fonts() -> RunFonts {
    RunFonts::new()
        .ascii("Segoe UI")
        .hi_ansi("Segoe UI")
        .east_asia("Segoe UI")
        .cs("Segoe UI")
}

fn monospace_fonts() -> RunFonts {
    RunFonts::new()
        .ascii("Consolas")
        .hi_ansi("Consolas")
        .east_asia("Consolas")
        .cs("Consolas")
}

/// Kern der Konvertierung: läuft einmal linear über den
/// `pulldown-cmark`-Event-Stream und baut dabei eine flache Liste fertiger
/// [`Paragraph`]s auf.
fn parse_markdown_to_paragraphs(content_markdown: &str) -> Vec<Paragraph> {
    let mut paragraphs = Vec::new();
    let mut current: Option<Paragraph> = None;
    let mut list_kinds: Vec<bool> = Vec::new(); // true = geordnet (nummeriert)
    let mut bold = false;
    let mut italic = false;
    let mut in_code_block = false;

    let flush = |current: &mut Option<Paragraph>, paragraphs: &mut Vec<Paragraph>| {
        if let Some(p) = current.take() {
            paragraphs.push(p);
        }
    };

    let add_run = |current: &mut Option<Paragraph>, run: Run| {
        let paragraph = current.take().unwrap_or_default();
        *current = Some(paragraph.add_run(run));
    };

    for event in Parser::new_ext(content_markdown, Options::empty()) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                flush(&mut current, &mut paragraphs);
                let spacing = match level {
                    HeadingLevel::H1 => LineSpacing::new().before(240).after(100).line(280),
                    HeadingLevel::H2 => LineSpacing::new().before(180).after(70).line(280),
                    _ => LineSpacing::new().before(140).after(50).line(280),
                };
                current = Some(
                    Paragraph::new()
                        .style(heading_style_id(level))
                        .line_spacing(spacing),
                );
            }
            Event::End(TagEnd::Heading(_)) => flush(&mut current, &mut paragraphs),

            Event::Start(Tag::CodeBlock(_)) => {
                flush(&mut current, &mut paragraphs);
                in_code_block = true;
                current = Some(
                    Paragraph::new()
                        .indent(Some(360), None, None, None)
                        .line_spacing(LineSpacing::new().before(60).after(80).line(240)),
                );
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                flush(&mut current, &mut paragraphs);
            }

            Event::Start(Tag::Paragraph) => {
                if current.is_none() {
                    current = Some(
                        Paragraph::new().line_spacing(LineSpacing::new().after(120).line(276)),
                    );
                }
            }
            Event::End(TagEnd::Paragraph) => flush(&mut current, &mut paragraphs),

            Event::Start(Tag::List(start)) => list_kinds.push(start.is_some()),
            Event::End(TagEnd::List(_)) => {
                list_kinds.pop();
            }
            Event::Start(Tag::Item) => {
                flush(&mut current, &mut paragraphs);
                let ordered = *list_kinds.last().unwrap_or(&false);
                let numbering_id = if ordered {
                    ORDERED_NUMBERING_ID
                } else {
                    BULLET_NUMBERING_ID
                };
                let depth = list_kinds
                    .len()
                    .saturating_sub(1)
                    .min(LIST_INDENT_LEVELS - 1);
                current = Some(
                    Paragraph::new()
                        .numbering(NumberingId::new(numbering_id), IndentLevel::new(depth))
                        .line_spacing(LineSpacing::new().after(50).line(260)),
                );
            }
            Event::End(TagEnd::Item) => flush(&mut current, &mut paragraphs),

            Event::Start(Tag::BlockQuote(_)) => {
                flush(&mut current, &mut paragraphs);
                current = Some(
                    Paragraph::new()
                        .indent(Some(360), None, None, None)
                        .italic()
                        .line_spacing(LineSpacing::new().after(100).line(276)),
                );
            }
            Event::End(TagEnd::BlockQuote(_)) => flush(&mut current, &mut paragraphs),

            Event::Start(Tag::Strong) => bold = true,
            Event::End(TagEnd::Strong) => bold = false,
            Event::Start(Tag::Emphasis) => italic = true,
            Event::End(TagEnd::Emphasis) => italic = false,

            Event::Text(text) => {
                let mut run = Run::new().add_text(text.into_string());
                if bold {
                    run = run.bold();
                }
                if italic {
                    run = run.italic();
                }
                if in_code_block {
                    run = run.fonts(monospace_fonts()).size(19);
                }
                add_run(&mut current, run);
            }
            Event::Code(text) => {
                let run = Run::new()
                    .add_text(text.into_string())
                    .fonts(monospace_fonts())
                    .size(20);
                add_run(&mut current, run);
            }
            Event::SoftBreak => add_run(&mut current, Run::new().add_text(" ")),
            Event::HardBreak => {
                add_run(&mut current, Run::new().add_break(BreakType::TextWrapping))
            }
            Event::Rule => flush(&mut current, &mut paragraphs),

            _ => {}
        }
    }
    flush(&mut current, &mut paragraphs);

    if paragraphs.is_empty() {
        paragraphs.push(Paragraph::new());
    }
    paragraphs
}

#[cfg(test)]
mod tests {
    use super::*;
    use docx_rs::DocumentChild;

    fn extract_text(bytes: &[u8]) -> String {
        let docx = docx_rs::read_docx(bytes).expect("erzeugtes DOCX muss lesbar/öffnbar sein");
        docx.document
            .children
            .iter()
            .filter_map(|child| match child {
                DocumentChild::Paragraph(p) => Some(p.raw_text()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn test_markdown_to_docx_bytes_produces_openable_docx_with_expected_text() {
        let markdown = "# Analyse\n\nDies ist ein **fetter** und *kursiver* Absatz.\n\n\
             ## Ergebnisse\n\n- Erster Punkt\n- Zweiter Punkt\n\n1. Schritt eins\n2. Schritt zwei\n";

        let bytes = markdown_to_docx_bytes(markdown);
        let text = extract_text(&bytes);

        assert!(text.contains("Analyse"));
        assert!(text.contains("fetter"));
        assert!(text.contains("kursiver"));
        assert!(text.contains("Ergebnisse"));
        assert!(text.contains("Erster Punkt"));
        assert!(text.contains("Schritt eins"));
    }

    #[test]
    fn test_markdown_to_docx_bytes_marks_heading_paragraph_with_heading_style() {
        let bytes = markdown_to_docx_bytes("# Titel\n\nText.");

        let docx = docx_rs::read_docx(&bytes).unwrap();
        let heading_paragraph = docx.document.children.iter().find_map(|child| match child {
            DocumentChild::Paragraph(p) if p.raw_text() == "Titel" => Some(p),
            _ => None,
        });

        assert!(
            heading_paragraph.is_some(),
            "Überschrift muss als eigener Absatz mit Text 'Titel' vorkommen"
        );
    }

    #[test]
    fn test_markdown_to_docx_bytes_renders_code_block_as_monospace_paragraph() {
        let markdown = "```\nfn main() {}\n```\n";

        let bytes = markdown_to_docx_bytes(markdown);
        let text = extract_text(&bytes);

        assert!(text.contains("fn main()"));
    }

    #[test]
    fn test_markdown_to_docx_bytes_handles_empty_content_without_panicking() {
        let bytes = markdown_to_docx_bytes("");

        assert!(docx_rs::read_docx(&bytes).is_ok());
    }

    #[test]
    fn test_default_export_file_name_uses_markdown_extension() {
        let name = default_export_file_name("Kurze Analyse", DocumentFormat::Markdown);

        assert_eq!(name, "Kurze Analyse.md");
    }

    #[test]
    fn test_default_export_file_name_uses_word_extension() {
        let name = default_export_file_name("Kurze Analyse", DocumentFormat::Word);

        assert_eq!(name, "Kurze Analyse.docx");
    }

    #[test]
    fn test_default_export_file_name_sanitizes_path_unsafe_characters() {
        let name = default_export_file_name("Bericht: Server/Log \"prod\"", DocumentFormat::Word);

        assert_eq!(name, "Bericht Server Log prod.docx");
    }

    #[test]
    fn test_default_export_file_name_falls_back_for_empty_title() {
        let name = default_export_file_name("   ", DocumentFormat::Markdown);

        assert_eq!(name, "Dokument.md");
    }
}
