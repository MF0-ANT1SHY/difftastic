use std::collections::BTreeMap;

use line_numbers::LineNumber;
use serde::{ser::SerializeStruct, Serialize, Serializer};

use crate::{
    display::{
        context::{all_matched_lines_filled, opposite_positions},
        hunks::{matched_pos_to_hunks, merge_adjacent}, // Still needed for status calculation
        side_by_side::lines_with_novel,
    },
    lines::MaxLine, // Still needed for status calculation
    parse::syntax::{self, MatchedPos, StringKind}, // MatchedPos and syntax might be less directly used for final JSON fields
    summary::{DiffResult, FileContent, FileFormat},
};

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Status {
    Unchanged,
    Changed,
    Created,
    Deleted,
}

// Modified File struct for the new output format
#[derive(Debug)]
struct File<'f> {
    language: &'f FileFormat,
    path: &'f str,
    status: Status,
    line_mapping: Vec<BTreeMap<u32, u32>>,
    lhs_content: Vec<BTreeMap<u32, &'f str>>,
}

impl<'f> File<'f> {
    // Helper to create File instance, typically used when there's no line-specific data to show
    fn with_status_empty(language: &'f FileFormat, path: &'f str, status: Status) -> File<'f> {
        File {
            language,
            path,
            status,
            line_mapping: Vec::new(),
            lhs_content: Vec::new(),
        }
    }
}

impl<'f> From<&'f DiffResult> for File<'f> {
    fn from(summary: &'f DiffResult) -> Self {
        match (&summary.lhs_src, &summary.rhs_src) {
            (FileContent::Text(lhs_src), FileContent::Text(rhs_src)) => {
                // Handle edge cases for file status
                if lhs_src.is_empty() && rhs_src.is_empty() {
                    return File::with_status_empty(&summary.file_format, &summary.display_path, Status::Unchanged);
                }
                if lhs_src.is_empty() {
                    return File::with_status_empty(&summary.file_format, &summary.display_path, Status::Created);
                }
                if rhs_src.is_empty() {
                    return File::with_status_empty(&summary.file_format, &summary.display_path, Status::Deleted);
                }

                let lhs_lines_vec = lhs_src.split('\n').collect::<Vec<&str>>();
                let rhs_lines_vec = rhs_src.split('\n').collect::<Vec<&str>>();

                let (lhs_lines_with_novel, rhs_lines_with_novel) =
                    lines_with_novel(&summary.lhs_positions, &summary.rhs_positions);

                let all_aligned_file_lines = all_matched_lines_filled(
                    &summary.lhs_positions,
                    &summary.rhs_positions,
                    &lhs_lines_vec,
                    &rhs_lines_vec,
                );

                let mut line_mapping_data = Vec::new();
                let mut lhs_content_data = Vec::new();

                for (lhs_line_num_opt, rhs_line_num_opt) in all_aligned_file_lines {
                    if let (Some(lhs_ln), Some(rhs_ln)) = (lhs_line_num_opt, rhs_line_num_opt) {
                        let is_lhs_novel = lhs_lines_with_novel.contains(&lhs_ln);
                        let is_rhs_novel = rhs_lines_with_novel.contains(&rhs_ln);

                        if !is_lhs_novel && !is_rhs_novel {
                            // This line is mapped AND unchanged.
                            // Line numbers are 0-based internally, convert to 1-based for output.
                            let lhs_one_based = lhs_ln.0 + 1;
                            let rhs_one_based = rhs_ln.0 + 1;

                            let mut mapping_entry = BTreeMap::new();
                            mapping_entry.insert(lhs_one_based, rhs_one_based);
                            line_mapping_data.push(mapping_entry);

                            if (lhs_ln.0 as usize) < lhs_lines_vec.len() {
                                let mut content_entry = BTreeMap::new();
                                content_entry.insert(lhs_one_based, lhs_lines_vec[lhs_ln.0 as usize]);
                                lhs_content_data.push(content_entry);
                            }
                        }
                    }
                }

                // Determine the overall file status based on whether any *changes*
                // were reported by the core diffing algorithm.
                let calculated_status = {
                    let change_hunks = matched_pos_to_hunks(&summary.lhs_positions, &summary.rhs_positions);
                    if change_hunks.is_empty() {
                        Status::Unchanged
                    } else {
                        let opposite_to_lhs = opposite_positions(&summary.lhs_positions);
                        let opposite_to_rhs = opposite_positions(&summary.rhs_positions);
                        let merged_hunks = merge_adjacent(
                            &change_hunks,
                            &opposite_to_lhs,
                            &opposite_to_rhs,
                            lhs_src.max_line(),
                            rhs_src.max_line(),
                            0, // context_lines = 0 for just checking if changes exist
                        );
                        if merged_hunks.is_empty() { Status::Unchanged } else { Status::Changed }
                    }
                };
                
                File {
                    language: &summary.file_format,
                    path: &summary.display_path,
                    status: calculated_status,
                    line_mapping: line_mapping_data,
                    lhs_content: lhs_content_data,
                }
            }
            (FileContent::Binary, FileContent::Binary) => {
                let status = if summary.has_byte_changes { Status::Changed } else { Status::Unchanged };
                File::with_status_empty(&FileFormat::Binary, &summary.display_path, status)
            }
            (_, FileContent::Binary) | (FileContent::Binary, _) => {
                File::with_status_empty(&FileFormat::Binary, &summary.display_path, Status::Changed)
            }
        }
    }
}

// Custom Serialize implementation for the modified File struct
impl Serialize for File<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut field_count = 3; // language, path, status are always present
        if !self.line_mapping.is_empty() {
            field_count += 1;
        }
        if !self.lhs_content.is_empty() {
            field_count += 1;
        }

        let mut file_serializer = serializer.serialize_struct("File", field_count)?;

        file_serializer.serialize_field("language", &format!("{}", self.language))?;
        file_serializer.serialize_field("path", &self.path)?;
        file_serializer.serialize_field("status", &self.status)?;

        if !self.line_mapping.is_empty() {
            file_serializer.serialize_field("line_mapping", &self.line_mapping)?;
        }
        if !self.lhs_content.is_empty() {
            file_serializer.serialize_field("lhs_content", &self.lhs_content)?;
        }

        file_serializer.end()
    }
}

// The structs below (Line, Side, Change, Highlight) are no longer directly
// used to populate the main fields of the JSON output in this new format.
// They are kept as they are part of the original module's definitions and
// might be used by other parts of the `difft` crate or for other output formats.
// If this `json.rs` module is *solely* for this specific JSON output,
// they (and their helper functions) could be removed.

#[derive(Debug, Serialize)]
struct Line<'l> {
    #[serde(skip_serializing_if = "Option::is_none")]
    lhs: Option<Side<'l>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rhs: Option<Side<'l>>,
}

impl<'l> Line<'l> {
    #[allow(dead_code)] // Potentially unused in the new direct JSON format
    fn new(lhs_number: Option<u32>, rhs_number: Option<u32>) -> Line<'l> {
        Line {
            lhs: lhs_number.map(Side::new),
            rhs: rhs_number.map(Side::new),
        }
    }
}

#[derive(Debug, Serialize)]
struct Side<'s> {
    line_number: u32,
    changes: Vec<Change<'s>>,
}

impl<'s> Side<'s> {
    #[allow(dead_code)] // Potentially unused
    fn new(line_number: u32) -> Side<'s> {
        Side {
            line_number,
            changes: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize)]
struct Change<'c> {
    start: u32,
    end: u32,
    content: &'c str,
    highlight: Highlight,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum Highlight {
    Delimiter,
    Normal,
    String,
    Type,
    Comment,
    Keyword,
    TreeSitterError,
}

impl Highlight {
    #[allow(dead_code)] // Potentially unused
    fn from_match(kind: &syntax::MatchKind) -> Self {
        use syntax::{AtomKind, MatchKind, TokenKind};

        let highlight = match kind {
            MatchKind::Ignored { highlight, .. } => highlight,
            MatchKind::UnchangedToken { highlight, .. } => highlight,
            MatchKind::Novel { highlight, .. } => highlight,
            MatchKind::NovelWord { highlight, .. } => highlight,
            MatchKind::UnchangedPartOfNovelItem { highlight, .. } => highlight,
        };

        match highlight {
            TokenKind::Delimiter => Highlight::Delimiter,
            TokenKind::Atom(atom) => match atom {
                AtomKind::String(StringKind::StringLiteral) => Highlight::String,
                AtomKind::String(StringKind::Text) => Highlight::Normal,
                AtomKind::Keyword => Highlight::Keyword,
                AtomKind::Comment => Highlight::Comment,
                AtomKind::Type => Highlight::Type,
                AtomKind::Normal => Highlight::Normal,
                AtomKind::TreeSitterError => Highlight::TreeSitterError,
            },
        }
    }
}

pub(crate) fn print_directory(diffs: Vec<DiffResult>, print_unchanged: bool) {
    let files = diffs
        .iter()
        .map(File::from) // File::from now uses the new logic
        .filter(|f| print_unchanged || f.status != Status::Unchanged)
        .collect::<Vec<File>>();
    println!(
        "{}",
        serde_json::to_string_pretty(&files).expect("failed to serialize files") // Used to_string_pretty for readability
    );
}

pub(crate) fn print(diff: &DiffResult) {
    let file = File::from(diff); // File::from now uses the new logic
    println!(
        "{}",
        serde_json::to_string_pretty(&file).expect("failed to serialize file") // Used to_string_pretty for readability
    )
}

#[allow(dead_code)] // No longer directly called in the new JSON generation path
fn add_changes_to_side<'s>(
    side: &mut Side<'s>,
    line_num: LineNumber,
    src_lines: &[&'s str],
    all_matches: &[MatchedPos],
) {
    let src_line = src_lines[line_num.0 as usize];

    let matches = matches_for_line(all_matches, line_num);
    for m in matches {
        side.changes.push(Change {
            start: m.pos.start_col,
            end: m.pos.end_col,
            content: &src_line[(m.pos.start_col as usize)..(m.pos.end_col as usize)],
            highlight: Highlight::from_match(&m.kind),
        })
    }
}

#[allow(dead_code)] // No longer directly called
fn matches_for_line(matches: &[MatchedPos], line_num: LineNumber) -> Vec<&MatchedPos> {
    matches
        .iter()
        .filter(|m| m.pos.line == line_num)
        .filter(|m| m.kind.is_novel())
        .collect()
}