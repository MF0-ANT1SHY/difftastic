use std::collections::BTreeMap;

use serde::Serialize;
use crate::{
    display::context::opposite_positions,
    summary::{DiffResult, FileContent},
};

/// Represents mapping between unchanged lines in both files
#[derive(Debug, Serialize)]
pub struct UnchangedLinesMapping {
    /// Path of the file being compared
    path: String,
    /// Mapping from lhs line numbers to rhs line numbers for unchanged lines
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    line_mappings: BTreeMap<u32, u32>,
    /// Overall file status
    status: FileStatus,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum FileStatus {
    Unchanged,
    Changed,
    Created,
    Deleted,
    Binary,
}

impl<'f> From<&'f DiffResult> for UnchangedLinesMapping {
    fn from(diff: &'f DiffResult) -> Self {
        match (&diff.lhs_src, &diff.rhs_src) {
            (FileContent::Text(lhs_src), FileContent::Text(rhs_src)) => {
                // File is created or deleted
                if lhs_src.is_empty() {
                    return UnchangedLinesMapping {
                        path: diff.display_path.clone(),
                        line_mappings: BTreeMap::new(),
                        status: FileStatus::Created,
                    };
                }
                if rhs_src.is_empty() {
                    return UnchangedLinesMapping {
                        path: diff.display_path.clone(),
                        line_mappings: BTreeMap::new(),
                        status: FileStatus::Deleted,
                    };
                }

                // Get opposites
                let lhs_opposite = opposite_positions(&diff.lhs_positions);
                let rhs_opposite = opposite_positions(&diff.rhs_positions);

                // Create line mappings for unchanged lines
                let mut line_mappings = BTreeMap::new();
                
                // Process matched positions to find unchanged lines
                for (i, lhs_pos) in diff.lhs_positions.iter().enumerate() {
                    if !lhs_pos.kind.is_novel() {
                        // If there is a corresponding position in rhs
                        if let Some(rhs_indices) = lhs_opposite.get(&lhs_pos.pos.line) {
                            for rhs_idx in rhs_indices.iter() {
                                // Find the corresponding rhs position 
                                for rhs_pos in diff.rhs_positions.iter() {
                                    if rhs_pos.pos.line == *rhs_idx && !rhs_pos.kind.is_novel() {
                                        // Add mapping from lhs line to rhs line
                                        line_mappings.insert(lhs_pos.pos.line.0, rhs_idx.0);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }

                let status = if line_mappings.is_empty() {
                    // If there are no mappings but both files have content, 
                    // everything is different
                    FileStatus::Changed
                } else if line_mappings.len() == count_lines(lhs_src) as usize 
                    && line_mappings.len() == count_lines(rhs_src) as usize {
                    // If all lines are mapped and count matches for both files, 
                    // files are identical
                    FileStatus::Unchanged
                } else {
                    // Some lines match, some don't
                    FileStatus::Changed
                };

                UnchangedLinesMapping {
                    path: diff.display_path.clone(),
                    line_mappings,
                    status,
                }
            },
            (FileContent::Binary, FileContent::Binary) => {
                let status = if diff.has_byte_changes {
                    FileStatus::Changed
                } else {
                    FileStatus::Unchanged
                };
                
                UnchangedLinesMapping {
                    path: diff.display_path.clone(),
                    line_mappings: BTreeMap::new(),
                    status,
                }
            },
            _ => UnchangedLinesMapping {
                path: diff.display_path.clone(),
                line_mappings: BTreeMap::new(),
                status: FileStatus::Binary,
            },
        }
    }
}

/// Count the number of lines in a string
fn count_lines(text: &str) -> u32 {
    // Count newlines and add 1 if the string isn't empty and doesn't end with a newline
    let newline_count = text.as_bytes().iter().filter(|&&b| b == b'\n').count();
    if text.is_empty() {
        0
    } else if text.ends_with('\n') {
        newline_count as u32
    } else {
        (newline_count + 1) as u32
    }
}

/// Output unchanged line mappings for a single diff result
pub fn print_unchanged_mappings(diff: &DiffResult) {
    let mapping = UnchangedLinesMapping::from(diff);
    println!(
        "{}",
        serde_json::to_string(&mapping).expect("Failed to serialize unchanged line mappings")
    );
}

/// Output unchanged line mappings for multiple diff results
pub fn print_all_unchanged_mappings(diffs: &[DiffResult], _print_unchanged: bool) {
    let mappings: Vec<UnchangedLinesMapping> = diffs.iter().map(UnchangedLinesMapping::from).collect();
    println!(
        "{}",
        serde_json::to_string(&mappings).expect("Failed to serialize unchanged line mappings")
    );
}

// Rename these to match the expected function names in the main file
pub fn print(diff: &DiffResult) {
    print_unchanged_mappings(diff);
}

pub fn print_directory(diffs: Vec<DiffResult>, print_unchanged: bool) {
    print_all_unchanged_mappings(&diffs, print_unchanged);
}