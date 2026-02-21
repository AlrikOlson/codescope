//! Convention mining — detect coding patterns from source files at scan time.
//!
//! Analyzes files via lightweight string matching to detect error handling style,
//! naming conventions, testing patterns, and import organization.

use crate::types::ScannedFile;
use serde::Serialize;
use std::fs;

// ---------------------------------------------------------------------------
// Convention report types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct ConventionReport {
    pub error_handling: ErrorHandlingConventions,
    pub naming: NamingConventions,
    pub testing: TestingConventions,
    pub import_style: ImportConventions,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorHandlingConventions {
    pub result_type_count: usize,
    pub unwrap_count: usize,
    pub try_catch_count: usize,
    pub style: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NamingConventions {
    pub snake_case_fns: usize,
    pub camel_case_fns: usize,
    pub pascal_case_types: usize,
    pub style: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TestingConventions {
    pub test_attribute_count: usize,
    pub test_file_count: usize,
    pub style: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportConventions {
    pub grouped_imports: bool,
    pub wildcard_imports: usize,
    pub style: String,
}

// ---------------------------------------------------------------------------
// Convention mining
// ---------------------------------------------------------------------------

/// Analyze all source files and detect conventions via string matching.
pub fn mine_conventions(files: &[ScannedFile]) -> ConventionReport {
    let mut result_count: usize = 0;
    let mut unwrap_count: usize = 0;
    let mut try_catch_count: usize = 0;
    let mut question_mark_count: usize = 0;

    let mut snake_case_fns: usize = 0;
    let mut camel_case_fns: usize = 0;
    let mut pascal_case_types: usize = 0;

    let mut test_attr_count: usize = 0;
    let mut test_file_count: usize = 0;
    let mut jest_count: usize = 0;
    let mut pytest_count: usize = 0;

    let mut wildcard_imports: usize = 0;
    let mut grouped_files: usize = 0;
    let mut ungrouped_files: usize = 0;

    for file in files {
        let content = match fs::read_to_string(&file.abs_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Track if this is a test file
        let is_test_file = file.rel_path.contains("test")
            || file.rel_path.contains("spec")
            || file.rel_path.ends_with("_test.go")
            || file.rel_path.ends_with("_test.rs");
        if is_test_file {
            test_file_count += 1;
        }

        let mut file_has_import_gap = false;
        let mut in_import_block = false;

        for line in content.lines() {
            let trimmed = line.trim();

            // Error handling patterns
            if trimmed.contains("Result<") || trimmed.contains("-> Result") {
                result_count += 1;
            }
            if trimmed.contains(".unwrap()") {
                unwrap_count += 1;
            }
            if trimmed.contains("try {") || trimmed.contains("try:") {
                try_catch_count += 1;
            }
            if trimmed.contains("catch ") || trimmed.contains("except ") {
                try_catch_count += 1;
            }
            if trimmed.ends_with('?') || trimmed.contains("?)") || trimmed.contains("?;") {
                question_mark_count += 1;
            }

            // Naming: detect function definitions
            if let Some(fn_name) = extract_fn_name(trimmed) {
                if is_snake_case(fn_name) {
                    snake_case_fns += 1;
                } else if is_camel_case(fn_name) {
                    camel_case_fns += 1;
                }
            }

            // Naming: detect type definitions
            if let Some(type_name) = extract_type_name(trimmed) {
                if is_pascal_case(type_name) {
                    pascal_case_types += 1;
                }
            }

            // Testing patterns
            if trimmed.contains("#[test]") || trimmed.contains("#[tokio::test]") {
                test_attr_count += 1;
            }
            if trimmed.starts_with("describe(") || trimmed.starts_with("it(") {
                jest_count += 1;
            }
            if trimmed.starts_with("def test_") || trimmed.contains("@pytest") {
                pytest_count += 1;
            }

            // Import style
            if trimmed.contains("use ") && trimmed.contains("::*") {
                wildcard_imports += 1;
            }
            if trimmed.starts_with("import ") && trimmed.contains('*') {
                wildcard_imports += 1;
            }

            // Import grouping detection
            let is_import_line = trimmed.starts_with("use ")
                || trimmed.starts_with("import ")
                || trimmed.starts_with("from ")
                || trimmed.starts_with("#include");
            if is_import_line {
                in_import_block = true;
            } else if in_import_block && trimmed.is_empty() {
                file_has_import_gap = true;
            } else if in_import_block && !trimmed.is_empty() && !is_import_line {
                in_import_block = false;
            }
        }

        if file_has_import_gap {
            grouped_files += 1;
        } else if in_import_block || content.lines().any(|l| {
            let t = l.trim();
            t.starts_with("use ") || t.starts_with("import ") || t.starts_with("from ")
        }) {
            ungrouped_files += 1;
        }
    }

    // Determine styles
    let error_style = if result_count + question_mark_count > try_catch_count * 2 {
        "result-based"
    } else if try_catch_count > result_count + question_mark_count {
        "exception-based"
    } else if result_count + question_mark_count + try_catch_count == 0 {
        "none detected"
    } else {
        "mixed"
    };

    let naming_style = if snake_case_fns > camel_case_fns * 3 {
        "snake_case"
    } else if camel_case_fns > snake_case_fns * 3 {
        "camelCase"
    } else if snake_case_fns + camel_case_fns == 0 {
        "none detected"
    } else {
        "mixed"
    };

    let test_style = if test_attr_count > 0 && jest_count == 0 && pytest_count == 0 {
        "rust-test"
    } else if jest_count > 0 && test_attr_count == 0 {
        "jest-style"
    } else if pytest_count > 0 && test_attr_count == 0 {
        "pytest-style"
    } else if test_attr_count + jest_count + pytest_count == 0 {
        "none"
    } else {
        "mixed"
    };

    let import_style = if grouped_files > ungrouped_files && grouped_files > 0 {
        "grouped"
    } else if ungrouped_files > grouped_files {
        "ungrouped"
    } else if grouped_files + ungrouped_files == 0 {
        "none detected"
    } else {
        "mixed"
    };

    ConventionReport {
        error_handling: ErrorHandlingConventions {
            result_type_count: result_count + question_mark_count,
            unwrap_count,
            try_catch_count,
            style: error_style.to_string(),
        },
        naming: NamingConventions {
            snake_case_fns,
            camel_case_fns,
            pascal_case_types,
            style: naming_style.to_string(),
        },
        testing: TestingConventions {
            test_attribute_count: test_attr_count + jest_count + pytest_count,
            test_file_count,
            style: test_style.to_string(),
        },
        import_style: ImportConventions {
            grouped_imports: grouped_files > ungrouped_files,
            wildcard_imports,
            style: import_style.to_string(),
        },
    }
}

/// Format a convention report as a human-readable summary.
pub fn format_conventions(report: &ConventionReport) -> String {
    let mut out = String::new();

    out.push_str("# Project Conventions\n\n");

    out.push_str("## Error Handling\n");
    out.push_str(&format!("- Style: {}\n", report.error_handling.style));
    out.push_str(&format!("- Result/? usage: {}\n", report.error_handling.result_type_count));
    out.push_str(&format!("- .unwrap() calls: {}\n", report.error_handling.unwrap_count));
    out.push_str(&format!("- try/catch blocks: {}\n", report.error_handling.try_catch_count));
    out.push('\n');

    out.push_str("## Naming\n");
    out.push_str(&format!("- Style: {}\n", report.naming.style));
    out.push_str(&format!("- snake_case functions: {}\n", report.naming.snake_case_fns));
    out.push_str(&format!("- camelCase functions: {}\n", report.naming.camel_case_fns));
    out.push_str(&format!("- PascalCase types: {}\n", report.naming.pascal_case_types));
    out.push('\n');

    out.push_str("## Testing\n");
    out.push_str(&format!("- Style: {}\n", report.testing.style));
    out.push_str(&format!("- Test attributes/markers: {}\n", report.testing.test_attribute_count));
    out.push_str(&format!("- Test files: {}\n", report.testing.test_file_count));
    out.push('\n');

    out.push_str("## Import Style\n");
    out.push_str(&format!("- Style: {}\n", report.import_style.style));
    out.push_str(&format!("- Grouped imports: {}\n", report.import_style.grouped_imports));
    out.push_str(&format!("- Wildcard imports: {}\n", report.import_style.wildcard_imports));

    out
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract a function name from a line, if it contains a function definition.
fn extract_fn_name(line: &str) -> Option<&str> {
    // Rust: fn name(
    // TypeScript/JS: function name( or name(
    // Python: def name(
    // Go: func name(
    let patterns: &[&str] = &["fn ", "function ", "def ", "func "];
    for pat in patterns {
        if let Some(idx) = line.find(pat) {
            let after = &line[idx + pat.len()..];
            let name = after.split(|c: char| !c.is_alphanumeric() && c != '_').next()?;
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

/// Extract a type/struct/class name from a line.
fn extract_type_name(line: &str) -> Option<&str> {
    let patterns: &[&str] = &["struct ", "class ", "enum ", "interface ", "type ", "trait "];
    for pat in patterns {
        if let Some(idx) = line.find(pat) {
            let after = &line[idx + pat.len()..];
            let name = after.split(|c: char| !c.is_alphanumeric() && c != '_').next()?;
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

fn is_snake_case(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_lowercase() || c.is_ascii_digit() || c == '_') && s.contains('_')
}

fn is_camel_case(s: &str) -> bool {
    if s.is_empty() || s.starts_with(|c: char| c.is_uppercase()) {
        return false;
    }
    s.chars().any(|c| c.is_uppercase())
}

fn is_pascal_case(s: &str) -> bool {
    !s.is_empty() && s.starts_with(|c: char| c.is_uppercase()) && s.chars().skip(1).any(|c| c.is_lowercase())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_fn_name() {
        assert_eq!(extract_fn_name("pub fn greet(name: &str)"), Some("greet"));
        assert_eq!(extract_fn_name("fn process_data()"), Some("process_data"));
        assert_eq!(extract_fn_name("function getName()"), Some("getName"));
        assert_eq!(extract_fn_name("def test_something():"), Some("test_something"));
        assert_eq!(extract_fn_name("let x = 5;"), None);
    }

    #[test]
    fn test_extract_type_name() {
        assert_eq!(extract_type_name("pub struct Config {"), Some("Config"));
        assert_eq!(extract_type_name("class App {"), Some("App"));
        assert_eq!(extract_type_name("pub enum Status {"), Some("Status"));
        assert_eq!(extract_type_name("interface AppConfig {"), Some("AppConfig"));
    }

    #[test]
    fn test_naming_detection() {
        assert!(is_snake_case("process_data"));
        assert!(!is_snake_case("processData"));
        assert!(!is_snake_case("ProcessData"));

        assert!(is_camel_case("processData"));
        assert!(!is_camel_case("process_data"));
        assert!(!is_camel_case("ProcessData"));

        assert!(is_pascal_case("ProcessData"));
        assert!(!is_pascal_case("processData"));
        assert!(!is_pascal_case("CONSTANT"));
    }

    #[test]
    fn test_mine_conventions_empty() {
        let report = mine_conventions(&[]);
        assert_eq!(report.error_handling.style, "none detected");
        assert_eq!(report.naming.style, "none detected");
        assert_eq!(report.testing.style, "none");
    }

    #[test]
    fn test_format_conventions() {
        let report = mine_conventions(&[]);
        let text = format_conventions(&report);
        assert!(text.contains("# Project Conventions"));
        assert!(text.contains("## Error Handling"));
        assert!(text.contains("## Naming"));
        assert!(text.contains("## Testing"));
        assert!(text.contains("## Import Style"));
    }
}
