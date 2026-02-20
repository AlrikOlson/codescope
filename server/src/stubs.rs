// ---------------------------------------------------------------------------
// Stub extraction — collapse function bodies, keep structure
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Language family classification
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LanguageFamily {
    BraceBased,
    IndentBased,
    ConfigIni,
    ConfigStructured,
    Unknown,
}

pub fn classify_language(ext: &str) -> LanguageFamily {
    match ext {
        // Brace-based languages
        "h" | "hpp" | "hxx" | "cpp" | "cxx" | "cc" | "c" | "cs" | "java" | "kt" | "scala"
        | "rs" | "go" | "js" | "ts" | "jsx" | "tsx" | "mjs" | "cjs" | "swift" | "usf" | "ush"
        | "hlsl" | "glsl" | "vert" | "frag" | "comp" | "wgsl" | "d" | "ps1" | "psm1" | "psd1" => {
            LanguageFamily::BraceBased
        }
        // Indent-based languages
        "py" | "rb" => LanguageFamily::IndentBased,
        // INI/CFG config
        "ini" | "cfg" | "conf" => LanguageFamily::ConfigIni,
        // Structured config (JSON, YAML, TOML, XML)
        "json" | "yaml" | "yml" | "toml" | "xml" => LanguageFamily::ConfigStructured,
        // Unknown
        _ => LanguageFamily::Unknown,
    }
}

// ---------------------------------------------------------------------------
// Main stub extraction entry point
// ---------------------------------------------------------------------------

/// Extract structural stubs from source code by language family.
/// Keeps: imports, macros, class/struct/enum/namespace declarations,
/// function signatures, member variables, type aliases.
/// Replaces: function/method bodies with `{ /* ... */ }`
pub fn extract_stubs(content: &str, ext: &str) -> String {
    match classify_language(ext) {
        LanguageFamily::ConfigIni => stub_ini(content),
        LanguageFamily::IndentBased => stub_python(content),
        LanguageFamily::ConfigStructured => stub_structured(content, ext),
        LanguageFamily::Unknown => stub_fallback(content),
        LanguageFamily::BraceBased => stub_brace_based(content),
    }
}

// ---------------------------------------------------------------------------
// Brace-based stub extraction (C/C++, Java, C#, Rust, Go, JS/TS, etc.)
// ---------------------------------------------------------------------------

fn stub_brace_based(content: &str) -> String {
    let mut out = String::with_capacity(content.len() / 3);
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;
    let mut brace_depth: i32 = 0;
    let mut scope_is_structural: Vec<bool> = Vec::new();
    let mut in_block_comment = false;
    let mut skip_until_close_brace: Option<i32> = None;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        if in_block_comment {
            if trimmed.contains("*/") {
                in_block_comment = false;
            }
            i += 1;
            continue;
        }

        if let Some(target) = skip_until_close_brace {
            for ch in line.chars() {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => {
                        brace_depth -= 1;
                        if brace_depth <= target {
                            skip_until_close_brace = None;
                            if !scope_is_structural.is_empty() {
                                scope_is_structural.pop();
                            }
                            break;
                        }
                    }
                    _ => {}
                }
            }
            i += 1;
            continue;
        }

        if trimmed.starts_with("/*") && !trimmed.contains("*/") {
            in_block_comment = true;
            i += 1;
            continue;
        }

        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.starts_with("//")
            || trimmed.starts_with("using ")
            || trimmed.starts_with("typedef ")
            || trimmed.starts_with("template")
            || trimmed.starts_with("friend ")
            || trimmed.starts_with("import ")
            || trimmed.starts_with("from ")
            || trimmed.starts_with("use ")
            || trimmed.starts_with("mod ")
            || trimmed.starts_with("extern ")
            || trimmed.starts_with("package ")
            || is_annotation_or_macro(trimmed)
        {
            out.push_str(line);
            out.push('\n');
            i += 1;
            continue;
        }

        let has_open = trimmed.contains('{');
        let has_close = trimmed.contains('}');

        if has_open {
            let is_structural = is_structural_scope(trimmed, &lines, i);

            if is_structural {
                out.push_str(line);
                out.push('\n');
                for ch in line.chars() {
                    match ch {
                        '{' => {
                            brace_depth += 1;
                            scope_is_structural.push(true);
                        }
                        '}' => {
                            brace_depth -= 1;
                            scope_is_structural.pop();
                        }
                        _ => {}
                    }
                }
            } else {
                let sig = line_before_brace(line);
                out.push_str(sig);
                out.push_str(" { /* ... */ }\n");

                if has_close && line.rfind('}').unwrap_or(0) > line.find('{').unwrap_or(0) {
                    // Single-line body — already stubbed
                } else {
                    let target_depth = brace_depth;
                    for ch in line.chars() {
                        match ch {
                            '{' => {
                                brace_depth += 1;
                                scope_is_structural.push(false);
                            }
                            '}' => {
                                brace_depth -= 1;
                                scope_is_structural.pop();
                            }
                            _ => {}
                        }
                    }
                    skip_until_close_brace = Some(target_depth);
                }
            }
            i += 1;
            continue;
        }

        if has_close {
            for ch in line.chars() {
                match ch {
                    '{' => {
                        brace_depth += 1;
                        scope_is_structural.push(true);
                    }
                    '}' => {
                        brace_depth -= 1;
                        scope_is_structural.pop();
                    }
                    _ => {}
                }
            }
            out.push_str(line);
            out.push('\n');
            i += 1;
            continue;
        }

        out.push_str(line);
        out.push('\n');
        i += 1;
    }

    // Remove excessive blank lines (3+ consecutive -> 2)
    let mut result = String::with_capacity(out.len());
    let mut blank_count = 0;
    for line in out.lines() {
        if line.trim().is_empty() {
            blank_count += 1;
            if blank_count <= 2 {
                result.push('\n');
            }
        } else {
            blank_count = 0;
            result.push_str(line);
            result.push('\n');
        }
    }

    result
}

/// Recognize annotations, macros, and attributes across languages.
pub fn is_annotation_or_macro(line: &str) -> bool {
    // Java/Kotlin annotations: @UpperCase
    if line.starts_with('@') && line.len() > 1 {
        let next = line.as_bytes()[1];
        if next.is_ascii_uppercase() {
            return true;
        }
    }

    // Rust attributes: #[...]
    if line.starts_with("#[") {
        return true;
    }

    // C# attributes: [UpperCase...] (not array indexing)
    if line.starts_with('[') && line.len() > 1 {
        let next = line.as_bytes()[1];
        if next.is_ascii_uppercase() {
            return true;
        }
    }

    // Go directives: //go:
    if line.starts_with("//go:") {
        return true;
    }

    // Generic ALL_CAPS_MACRO( pattern
    let bytes = line.as_bytes();
    if !bytes.is_empty() && bytes[0].is_ascii_uppercase() {
        if let Some(paren) = line.find('(') {
            let before = &line[..paren];
            if before.chars().all(|c| c.is_ascii_uppercase() || c == '_') && before.len() >= 3 {
                return true;
            }
        }
    }

    false
}

fn is_structural_scope(line: &str, lines: &[&str], idx: usize) -> bool {
    let check = |s: &str| -> bool {
        let t = s.trim();
        if t.starts_with("class ")
            || t.starts_with("struct ")
            || t.starts_with("namespace ")
            || t.starts_with("enum ")
            || t.starts_with("union ")
            || t.starts_with("interface ")
            || t.starts_with("trait ")
            || t.starts_with("impl ")
            || t.starts_with("module ")
            || t.starts_with("package ")
            || t.starts_with("object ")
            || t.contains("class ") && t.contains('{')
            || t.contains("struct ") && t.contains('{')
            || t.contains("namespace ") && t.contains('{')
            || t.contains("enum ") && t.contains('{')
            || t.contains("interface ") && t.contains('{')
            || t.contains("trait ") && t.contains('{')
            || t.contains("impl ") && t.contains('{')
        {
            return true;
        }
        if t.starts_with("extern ") {
            return true;
        }
        false
    };

    if check(line) {
        return true;
    }

    let trimmed = line.trim();
    if trimmed == "{" || trimmed.starts_with("{ ") || trimmed == "{}" {
        let mut j = idx.saturating_sub(1);
        while j > 0 && lines[j].trim().is_empty() {
            j -= 1;
        }
        // Walk past C++ inheritance continuation lines (: public Base, , public Other)
        while j > 0 {
            let lt = lines[j].trim();
            if lt.starts_with(',') || (lt.starts_with(':') && !lt.starts_with("::")) {
                j -= 1;
                while j > 0 && lines[j].trim().is_empty() {
                    j -= 1;
                }
            } else {
                break;
            }
        }
        if j < idx {
            return check(lines[j]);
        }
    }

    let before = line_before_brace(line).trim().to_string();
    if before.ends_with(')')
        || before.ends_with("const")
        || before.ends_with("override")
        || before.ends_with("final")
        || before.ends_with("noexcept")
        || before.ends_with("= 0")
        || before.ends_with("= default")
        || before.ends_with("= delete")
    {
        return false;
    }

    // Function with initializer list
    if before.contains(") :") || before.contains("):") {
        return false;
    }

    // Lambda
    if before.contains(']') && before.contains('(') {
        return false;
    }

    // Check for function keywords that indicate a non-structural scope
    let trimmed_before = before.trim();
    if trimmed_before.starts_with("fn ")
        || trimmed_before.starts_with("func ")
        || trimmed_before.starts_with("function ")
        || trimmed_before.contains(" fn ")
        || trimmed_before.contains(" func ")
        || trimmed_before.contains(" function ")
    {
        return false;
    }

    if before.ends_with('=') || before.ends_with("= ") {
        return true;
    }

    true
}

fn line_before_brace(line: &str) -> &str {
    match line.find('{') {
        Some(pos) => line[..pos].trim_end(),
        None => line,
    }
}

// ---------------------------------------------------------------------------
// INI stub extraction
// ---------------------------------------------------------------------------

fn stub_ini(content: &str) -> String {
    let mut out = String::new();
    let mut entries_in_section = 0;
    let max_entries = 5;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with('#') {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if trimmed.starts_with('[') {
            entries_in_section = 0;
            out.push_str(line);
            out.push('\n');
            continue;
        }
        entries_in_section += 1;
        if entries_in_section <= max_entries {
            out.push_str(line);
            out.push('\n');
        } else if entries_in_section == max_entries + 1 {
            out.push_str("; ... (more entries)\n");
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Python stub extraction
// ---------------------------------------------------------------------------

fn stub_python(content: &str) -> String {
    let mut out = String::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;
    let mut skip_body = false;
    let mut body_indent = 0;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();
        let indent = line.len() - line.trim_start().len();

        if skip_body {
            if !trimmed.is_empty() && indent <= body_indent {
                skip_body = false;
            } else {
                if trimmed.starts_with("\"\"\"") || trimmed.starts_with("'''") {
                    out.push_str(line);
                    out.push('\n');
                }
                i += 1;
                continue;
            }
        }

        if trimmed.starts_with("import ")
            || trimmed.starts_with("from ")
            || trimmed.starts_with('#')
            || trimmed.is_empty()
            || trimmed.starts_with('@')
        {
            out.push_str(line);
            out.push('\n');
        } else if trimmed.starts_with("class ")
            || trimmed.starts_with("def ")
            || trimmed.starts_with("async def ")
        {
            out.push_str(line);
            out.push('\n');
            if trimmed.starts_with("def ") || trimmed.starts_with("async def ") {
                body_indent = indent;
                skip_body = true;
                out.push_str(&" ".repeat(indent + 4));
                out.push_str("...\n");
            }
        } else if indent == 0 {
            out.push_str(line);
            out.push('\n');
        }

        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// Structured config stub extraction (JSON, YAML, TOML, XML)
// ---------------------------------------------------------------------------

fn stub_structured(content: &str, ext: &str) -> String {
    match ext {
        "json" => stub_json(content),
        "yaml" | "yml" => stub_yaml(content),
        "toml" => stub_toml(content),
        "xml" => stub_xml(content),
        _ => stub_fallback(content),
    }
}

/// JSON: return keys to depth 2
fn stub_json(content: &str) -> String {
    // Simple approach: parse with serde_json, extract top-level structure
    match serde_json::from_str::<serde_json::Value>(content) {
        Ok(val) => {
            let mut out = String::new();
            format_json_depth(&val, &mut out, 0, 2);
            out
        }
        Err(_) => stub_fallback(content),
    }
}

fn format_json_depth(val: &serde_json::Value, out: &mut String, depth: usize, max_depth: usize) {
    let indent = "  ".repeat(depth);
    match val {
        serde_json::Value::Object(map) => {
            out.push_str("{\n");
            for (i, (key, value)) in map.iter().enumerate() {
                out.push_str(&"  ".repeat(depth + 1));
                out.push_str(&format!("\"{key}\": "));
                if depth + 1 >= max_depth {
                    match value {
                        serde_json::Value::Object(_) => out.push_str("{...}"),
                        serde_json::Value::Array(a) => {
                            out.push_str(&format!("[...{} items]", a.len()))
                        }
                        _ => out.push_str(&value.to_string()),
                    }
                } else {
                    format_json_depth(value, out, depth + 1, max_depth);
                }
                if i < map.len() - 1 {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&indent);
            out.push('}');
        }
        serde_json::Value::Array(arr) => {
            out.push_str(&format!("[...{} items]", arr.len()));
        }
        _ => out.push_str(&val.to_string()),
    }
}

/// YAML: return top-level keys and their immediate children
fn stub_yaml(content: &str) -> String {
    let mut out = String::new();
    for line in content.lines() {
        // Top-level lines (no leading whitespace) or first-level children (2-space indent)
        if (!line.starts_with(' ') && !line.starts_with('\t'))
            || (line.starts_with("  ") && !line.starts_with("    "))
            || (line.starts_with('\t') && !line.starts_with("\t\t"))
        {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// TOML: return section headers + first 5 keys per section
fn stub_toml(content: &str) -> String {
    let mut out = String::new();
    let mut entries_in_section = 0;
    let max_entries = 5;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if trimmed.starts_with('[') {
            entries_in_section = 0;
            out.push_str(line);
            out.push('\n');
            continue;
        }
        entries_in_section += 1;
        if entries_in_section <= max_entries {
            out.push_str(line);
            out.push('\n');
        } else if entries_in_section == max_entries + 1 {
            out.push_str("# ... (more entries)\n");
        }
    }
    out
}

/// XML: return first 100 lines
fn stub_xml(content: &str) -> String {
    let lines: Vec<&str> = content.lines().take(100).collect();
    let mut out = lines.join("\n");
    if content.lines().count() > 100 {
        out.push_str("\n<!-- ... (truncated) -->\n");
    } else {
        out.push('\n');
    }
    out
}

// ---------------------------------------------------------------------------
// Fallback: first 100 lines as-is
// ---------------------------------------------------------------------------

fn stub_fallback(content: &str) -> String {
    let lines: Vec<&str> = content.lines().take(100).collect();
    let mut out = lines.join("\n");
    if content.lines().count() > 100 {
        out.push_str("\n// ... (truncated at 100 lines)\n");
    } else {
        out.push('\n');
    }
    out
}

// ---------------------------------------------------------------------------
// Tier extractors — progressive detail reduction
// ---------------------------------------------------------------------------

/// Tier 2: Minified stubs — strip comments, collapse blanks, limit includes/imports
#[allow(dead_code)]
pub fn extract_tier2(tier1: &str) -> String {
    let mut out = String::with_capacity(tier1.len() / 2);
    let mut includes_seen = 0u32;
    let mut prev_blank = false;

    for line in tier1.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("//") {
            continue;
        }

        if trimmed.starts_with("#include")
            || trimmed.starts_with("import ")
            || trimmed.starts_with("from ")
            || trimmed.starts_with("use ")
        {
            includes_seen += 1;
            if includes_seen <= 5 {
                out.push_str(trimmed);
                out.push('\n');
            } else if includes_seen == 6 {
                out.push_str("// ... more imports\n");
            }
            prev_blank = false;
            continue;
        }

        if trimmed.is_empty() {
            if !prev_blank {
                out.push('\n');
                prev_blank = true;
            }
            continue;
        }
        prev_blank = false;

        let indent = line.len() - line.trim_start().len();
        let tabs = indent / 4;
        for _ in 0..tabs {
            out.push('\t');
        }
        out.push_str(trimmed);
        out.push('\n');
    }
    out
}

/// Tier 3: Table of contents — one line per class/struct/function
#[allow(dead_code)]
pub fn extract_tier3(content: &str, ext: &str) -> String {
    match classify_language(ext) {
        LanguageFamily::ConfigIni => return extract_toc_ini(content),
        LanguageFamily::IndentBased => return extract_toc_python(content),
        _ => {}
    }

    let mut out = String::new();
    let mut current_class: Option<&str> = None;

    for line in content.lines() {
        let trimmed = line.trim();
        let indent = line.len() - line.trim_start().len();

        let is_type_decl = trimmed.starts_with("class ")
            || trimmed.starts_with("struct ")
            || trimmed.starts_with("enum ")
            || trimmed.starts_with("namespace ")
            || trimmed.starts_with("interface ")
            || trimmed.starts_with("trait ")
            || trimmed.starts_with("impl ");

        if is_type_decl {
            let decl = trimmed.split('{').next().unwrap_or(trimmed).trim();
            if decl.ends_with(';') {
                continue;
            }
            out.push_str(decl);
            out.push('\n');
            if trimmed.starts_with("class ") || trimmed.starts_with("struct ") {
                current_class = Some(decl);
            }
            continue;
        }

        // Function signatures — exclude control flow and generic ALL_CAPS calls
        // that aren't type declarations
        if trimmed.contains('(')
            && !trimmed.starts_with("if ")
            && !trimmed.starts_with("for ")
            && !trimmed.starts_with("while ")
            && !trimmed.starts_with("switch ")
            && !trimmed.starts_with("//")
            && !trimmed.starts_with('#')
            && !is_all_caps_call(trimmed)
        {
            let sig = trimmed.split('{').next().unwrap_or(trimmed).trim();
            if sig.contains('(')
                && (sig.ends_with(')')
                    || sig.ends_with("const")
                    || sig.ends_with("override")
                    || sig.ends_with("= 0")
                    || sig.ends_with("final"))
            {
                let pfx = if current_class.is_some() { "  " } else { "" };
                out.push_str(pfx);
                out.push_str(sig);
                out.push('\n');
            }
        }

        if (trimmed == "}" || trimmed.starts_with("};")) && indent == 0 {
            current_class = None;
        }
    }
    out
}

/// Check if a line is an ALL_CAPS_FUNCTION_CALL that isn't a type declaration.
/// Used to exclude logging macros, assertion macros, etc. from tier3.
fn is_all_caps_call(line: &str) -> bool {
    if let Some(paren) = line.find('(') {
        let before = line[..paren].trim();
        if !before.is_empty()
            && before.chars().all(|c| c.is_ascii_uppercase() || c == '_')
            && before.len() >= 3
        {
            return true;
        }
    }
    false
}

/// Tier 4: Manifest line — just path and description
pub fn extract_tier4(rel_path: &str, desc: &str) -> String {
    format!("// {rel_path} — {desc}\n")
}

fn extract_toc_ini(content: &str) -> String {
    let mut out = String::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            out.push_str(trimmed);
            out.push('\n');
        }
    }
    out
}

fn extract_toc_python(content: &str) -> String {
    let mut out = String::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("class ")
            || trimmed.starts_with("def ")
            || trimmed.starts_with("async def ")
        {
            let indent = line.len() - line.trim_start().len();
            for _ in 0..(indent / 4) {
                out.push_str("  ");
            }
            out.push_str(trimmed.split(':').next().unwrap_or(trimmed));
            out.push('\n');
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Block-level parsing for intra-file budget pruning
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BlockKind {
    IncludeGroup,
    AnnotatedBlock,
    ClassDecl,
    FunctionSig,
    MacroDecl,
    Misc,
}

#[derive(Debug, Clone)]
pub struct StubBlock {
    pub kind: BlockKind,
    pub identifier: String,
    pub full_text: String,
    pub summary_text: String,
    pub full_tokens: usize,
    pub summary_tokens: usize,
}

/// Parse tier1 stubs into discrete blocks for intra-file budget pruning.
/// Each block represents a logical unit (class, function, include group, etc.)
/// that can be independently kept, summarized, or dropped.
pub fn parse_blocks(tier1: &str, ext: &str) -> Vec<StubBlock> {
    // Non-brace-based files: single Misc block (block pruning mainly benefits brace-based)
    match classify_language(ext) {
        LanguageFamily::BraceBased => {}
        _ => {
            let tokens = tier1.len().div_ceil(3);
            return vec![StubBlock {
                kind: BlockKind::Misc,
                identifier: String::new(),
                full_text: tier1.to_string(),
                summary_text: tier1.to_string(),
                full_tokens: tokens,
                summary_tokens: tokens,
            }];
        }
    }

    let mut blocks: Vec<StubBlock> = Vec::new();
    let lines: Vec<&str> = tier1.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        // Skip blank lines
        if trimmed.is_empty() {
            i += 1;
            continue;
        }

        // --- Include/import group: contiguous import lines ---
        if trimmed.starts_with("#include")
            || trimmed.starts_with("#pragma")
            || trimmed.starts_with("import ")
            || trimmed.starts_with("from ")
            || (trimmed.starts_with("use ") && !trimmed.contains('{'))
        {
            let mut include_lines: Vec<&str> = Vec::new();
            while i < lines.len() {
                let t = lines[i].trim();
                if t.starts_with("#include")
                    || t.starts_with("#pragma")
                    || t.starts_with("import ")
                    || t.starts_with("from ")
                    || (t.starts_with("use ") && !t.contains('{'))
                {
                    include_lines.push(lines[i]);
                    i += 1;
                } else if t.is_empty() {
                    i += 1;
                } else {
                    break;
                }
            }
            let full_text = include_lines.join("\n") + "\n";
            let count = include_lines.len();
            let first_few: Vec<&str> = include_lines
                .iter()
                .take(3)
                .filter_map(|l| {
                    let t = l.trim();
                    // Extract the imported name from various formats
                    t.strip_prefix("#include")
                        .map(|s| {
                            s.trim().trim_matches('"').trim_matches('<').trim_matches('>').trim()
                        })
                        .and_then(|s| s.rsplit('/').next())
                        .or(Some(t))
                })
                .collect();
            let summary_text = format!("// {} imports ({})\n", count, first_few.join(", "));
            blocks.push(StubBlock {
                kind: BlockKind::IncludeGroup,
                identifier: String::new(),
                summary_text: summary_text.clone(),
                summary_tokens: summary_text.len().div_ceil(3),
                full_tokens: full_text.len().div_ceil(3),
                full_text,
            });
            continue;
        }

        // --- IMPLEMENT_* / DECLARE_* macros ---
        if trimmed.starts_with("IMPLEMENT_") || trimmed.starts_with("DECLARE_") {
            let name = block_macro_arg(trimmed);
            let mut block_text = lines[i].to_string();
            if !trimmed.ends_with(';') && !trimmed.ends_with(')') {
                i += 1;
                while i < lines.len() {
                    block_text.push('\n');
                    block_text.push_str(lines[i]);
                    let t = lines[i].trim();
                    if t.ends_with(';') || t.ends_with(')') || t == ")" {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            } else {
                i += 1;
            }
            block_text.push('\n');
            let tokens = block_text.len().div_ceil(3);
            blocks.push(StubBlock {
                kind: BlockKind::MacroDecl,
                identifier: name.to_lowercase(),
                full_text: block_text.clone(),
                summary_text: block_text,
                full_tokens: tokens,
                summary_tokens: tokens,
            });
            continue;
        }

        // --- Capitalized macro pattern: ^[A-Z][A-Z_]+\s*\( as MacroDecl ---
        if !trimmed.is_empty() && trimmed.as_bytes()[0].is_ascii_uppercase() {
            if let Some(paren) = trimmed.find('(') {
                let before = &trimmed[..paren];
                if before.len() >= 3
                    && before.chars().all(|c| c.is_ascii_uppercase() || c == '_')
                    && !trimmed.starts_with("IMPLEMENT_")
                    && !trimmed.starts_with("DECLARE_")
                {
                    let name = block_macro_arg(trimmed);
                    let mut block_lines = vec![lines[i]];
                    // If multi-line, collect until closing paren/brace
                    if !trimmed.ends_with(';') && !trimmed.ends_with(')') && !trimmed.ends_with('}')
                    {
                        let start_i = i;
                        i += 1;
                        while i < lines.len() && i < start_i + 20 {
                            block_lines.push(lines[i]);
                            let t = lines[i].trim();
                            if t.ends_with(';') || t.ends_with(')') || t.ends_with('}') || t == ")"
                            {
                                i += 1;
                                break;
                            }
                            i += 1;
                        }
                    } else {
                        i += 1;
                    }
                    let full_text = block_lines.join("\n") + "\n";
                    let tokens = full_text.len().div_ceil(3);
                    blocks.push(StubBlock {
                        kind: BlockKind::MacroDecl,
                        identifier: name.to_lowercase(),
                        full_text: full_text.clone(),
                        summary_text: full_text,
                        full_tokens: tokens,
                        summary_tokens: tokens,
                    });
                    continue;
                }
            }
        }

        // --- Annotated block: lines with annotations/attributes followed by a declaration ---
        if is_annotation_or_macro(trimmed)
            && !trimmed.starts_with("IMPLEMENT_")
            && !trimmed.starts_with("DECLARE_")
        {
            // Annotations that precede a class/struct/function: collect them
            let mut annotation_lines = vec![lines[i]];
            i += 1;
            while i < lines.len() && is_annotation_or_macro(lines[i].trim()) {
                annotation_lines.push(lines[i]);
                i += 1;
            }
            // The next line(s) should be the declaration — handled by normal flow
            let full_text = annotation_lines.join("\n") + "\n";
            let tokens = full_text.len().div_ceil(3);
            blocks.push(StubBlock {
                kind: BlockKind::AnnotatedBlock,
                identifier: String::new(),
                full_text: full_text.clone(),
                summary_text: full_text,
                full_tokens: tokens,
                summary_tokens: tokens,
            });
            continue;
        }

        // --- Class/struct declaration with body ---
        if (trimmed.starts_with("class ")
            || trimmed.starts_with("struct ")
            || trimmed.starts_with("interface ")
            || trimmed.starts_with("trait ")
            || trimmed.starts_with("impl "))
            && !trimmed.ends_with(';')
        {
            let name = block_class_name(trimmed);
            let mut brace_depth: i32 = 0;
            let mut block_lines: Vec<&str> = Vec::new();
            let mut member_count = 0u32;
            let mut found_open = false;

            loop {
                if i >= lines.len() {
                    break;
                }
                let line = lines[i];
                block_lines.push(line);
                for ch in line.chars() {
                    match ch {
                        '{' => {
                            brace_depth += 1;
                            found_open = true;
                        }
                        '}' => {
                            brace_depth -= 1;
                        }
                        _ => {}
                    }
                }
                let t = line.trim();
                if found_open
                    && brace_depth > 0
                    && !t.is_empty()
                    && !t.starts_with('{')
                    && !t.starts_with('}')
                {
                    member_count += 1;
                }
                i += 1;
                if found_open && brace_depth <= 0 {
                    break;
                }
            }

            let full_text = block_lines.join("\n") + "\n";
            let decl_line =
                block_lines[0].trim().split('{').next().unwrap_or(block_lines[0]).trim();
            let summary_text = format!("{} {{ /* {} members */ }};\n", decl_line, member_count);
            blocks.push(StubBlock {
                kind: BlockKind::ClassDecl,
                identifier: name.to_lowercase(),
                summary_text: summary_text.clone(),
                summary_tokens: summary_text.len().div_ceil(3),
                full_tokens: full_text.len().div_ceil(3),
                full_text,
            });
            continue;
        }

        // --- Function signature (ends with { /* ... */ }) ---
        if trimmed.ends_with("{ /* ... */ }") && trimmed.contains('(') {
            let name = block_function_name(trimmed);
            let full_text = format!("{}\n", lines[i]);
            let tokens = full_text.len().div_ceil(3);
            blocks.push(StubBlock {
                kind: BlockKind::FunctionSig,
                identifier: name.to_lowercase(),
                full_text: full_text.clone(),
                summary_text: full_text,
                full_tokens: tokens,
                summary_tokens: tokens,
            });
            i += 1;
            continue;
        }

        // --- Multi-line function sig: ( on this line, { /* ... */ } on a nearby line ---
        if trimmed.contains('(') && !trimmed.starts_with("//") && !trimmed.starts_with('#') {
            let mut j = i + 1;
            let mut found_stub = false;
            while j < lines.len() && j <= i + 4 {
                let t = lines[j].trim();
                if t.ends_with("{ /* ... */ }") {
                    let func_lines: Vec<&str> = (i..=j).map(|k| lines[k]).collect();
                    let name = block_function_name(trimmed);
                    let full_text = func_lines.join("\n") + "\n";
                    let tokens = full_text.len().div_ceil(3);
                    blocks.push(StubBlock {
                        kind: BlockKind::FunctionSig,
                        identifier: name.to_lowercase(),
                        full_text: full_text.clone(),
                        summary_text: full_text,
                        full_tokens: tokens,
                        summary_tokens: tokens,
                    });
                    i = j + 1;
                    found_stub = true;
                    break;
                }
                if !t.is_empty()
                    && !t.starts_with("//")
                    && !t.contains('(')
                    && !t.ends_with(')')
                    && !t.ends_with("const")
                    && !t.ends_with("override")
                {
                    break;
                }
                j += 1;
            }
            if found_stub {
                continue;
            }
        }

        // --- Misc: accumulate until next recognized block start ---
        let mut misc_lines = vec![lines[i]];
        i += 1;
        while i < lines.len() {
            let t = lines[i].trim();
            if t.starts_with("#include")
                || t.starts_with("#pragma")
                || t.starts_with("import ")
                || t.starts_with("from ")
                || t.starts_with("class ")
                || t.starts_with("struct ")
                || t.starts_with("interface ")
                || t.starts_with("trait ")
                || t.starts_with("impl ")
                || t.starts_with("IMPLEMENT_")
                || t.starts_with("DECLARE_")
                || (t.contains('(') && t.ends_with("{ /* ... */ }"))
            {
                break;
            }
            // Check for capitalized macro pattern
            if !t.is_empty() && t.as_bytes()[0].is_ascii_uppercase() {
                if let Some(paren) = t.find('(') {
                    let before = &t[..paren];
                    if before.len() >= 3
                        && before.chars().all(|c| c.is_ascii_uppercase() || c == '_')
                    {
                        break;
                    }
                }
            }
            misc_lines.push(lines[i]);
            i += 1;
        }

        let full_text = misc_lines.join("\n") + "\n";
        if !full_text.trim().is_empty() {
            let full_tokens = full_text.len().div_ceil(3);
            blocks.push(StubBlock {
                kind: BlockKind::Misc,
                identifier: String::new(),
                full_text,
                summary_text: String::new(),
                full_tokens,
                summary_tokens: 0,
            });
        }
    }

    blocks
}

fn block_macro_arg(line: &str) -> String {
    if let Some(start) = line.find('(') {
        let rest = &line[start + 1..];
        if let Some(end) = rest.find(&[',', ')'][..]) {
            return rest[..end].trim().to_string();
        }
    }
    String::new()
}

fn block_class_name(line: &str) -> String {
    let trimmed = line.trim();
    let after = if let Some(s) = trimmed.strip_prefix("class ") {
        s
    } else if let Some(s) = trimmed.strip_prefix("struct ") {
        s
    } else if let Some(s) = trimmed.strip_prefix("interface ") {
        s
    } else if let Some(s) = trimmed.strip_prefix("trait ") {
        s
    } else if let Some(s) = trimmed.strip_prefix("impl ") {
        s
    } else {
        trimmed
    };
    let end = after.find(&[' ', ':', '{', '\t', '<'][..]).unwrap_or(after.len());
    after[..end].trim().to_string()
}

fn block_function_name(line: &str) -> String {
    let trimmed = line.trim();
    if let Some(paren) = trimmed.find('(') {
        let before = &trimmed[..paren];
        if let Some(space) = before.rfind(&[' ', '\t', '*', '&', '>'][..]) {
            return before[space + 1..].trim().to_string();
        }
        return before.trim().to_string();
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multiline_class_declaration_preserved() {
        let input = "class FSlateApplication\n\t: public FSlateApplicationBase\n\t, public FGenericApplicationMessageHandler\n{\npublic:\n\tvoid Tick(float DeltaTime) { /* body */ }\n\tvirtual void OnKeyDown(int Key);\n\tint32 GetCursorPos() const { return CursorPos; }\nprivate:\n\tint32 CursorPos;\n};";
        let stubs = stub_brace_based(input);
        assert!(
            stubs.contains("void Tick("),
            "Method Tick should be preserved in stubs, got:\n{stubs}"
        );
        assert!(
            stubs.contains("void OnKeyDown("),
            "Method OnKeyDown should be preserved, got:\n{stubs}"
        );
        assert!(
            stubs.contains("GetCursorPos()"),
            "Method GetCursorPos should be preserved, got:\n{stubs}"
        );
        assert!(
            stubs.contains("int32 CursorPos"),
            "Member variable should be preserved, got:\n{stubs}"
        );
        assert!(stubs.contains("public:"), "Access specifier should be preserved, got:\n{stubs}");
    }

    #[test]
    fn test_single_line_class_preserved() {
        let input = "class Foo : public Bar {\npublic:\n\tvoid DoThing();\n\tint x;\n};";
        let stubs = stub_brace_based(input);
        assert!(stubs.contains("void DoThing()"), "Method should be preserved, got:\n{stubs}");
        assert!(stubs.contains("int x"), "Member should be preserved, got:\n{stubs}");
    }

    #[test]
    fn test_constructor_init_list_not_structural() {
        let input = "class Foo {\n\tFoo()\n\t\t: bar(1)\n\t\t, baz(2)\n\t{\n\t\tDoStuff();\n\t}\n\tint bar;\n\tint baz;\n};";
        let stubs = stub_brace_based(input);
        assert!(
            !stubs.contains("DoStuff()"),
            "Constructor body should be collapsed, got:\n{stubs}"
        );
        assert!(stubs.contains("int bar"), "Member should be preserved, got:\n{stubs}");
    }
}
