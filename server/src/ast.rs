//! tree-sitter AST parsing for precise symbol extraction.
//!
//! Extracts function, class, struct, trait, enum, and method definitions with
//! exact line boundaries from source files in 8 languages. Feature-gated behind
//! `treesitter` to keep the default binary lean.

use rayon::prelude::*;
use std::collections::HashMap;
use tree_sitter::{Language, Node, Parser};
use tracing::debug;

use crate::types::ScannedFile;

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// The kind of a code symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Struct,
    Enum,
    Interface,
    Trait,
    Impl,
    TypeAlias,
    Constant,
}

impl SymbolKind {
    pub fn label(&self) -> &'static str {
        match self {
            SymbolKind::Function => "fn",
            SymbolKind::Method => "method",
            SymbolKind::Class => "class",
            SymbolKind::Struct => "struct",
            SymbolKind::Enum => "enum",
            SymbolKind::Interface => "interface",
            SymbolKind::Trait => "trait",
            SymbolKind::Impl => "impl",
            SymbolKind::TypeAlias => "type",
            SymbolKind::Constant => "const",
        }
    }
}

/// A single extracted symbol with its location and metadata.
#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    /// 1-based start line.
    pub start_line: usize,
    /// 1-based end line (inclusive).
    pub end_line: usize,
    /// Index of parent symbol (e.g., method's class/impl), or None for top-level.
    pub parent_idx: Option<usize>,
    /// One-line display signature (e.g., "pub fn greet(name: &str) -> String").
    pub signature: String,
}

/// All symbols extracted from a single file.
#[derive(Debug, Clone)]
pub struct FileAst {
    pub symbols: Vec<Symbol>,
    /// Name → indices into `symbols` for fast lookup.
    pub name_index: HashMap<String, Vec<usize>>,
}

impl FileAst {
    fn new() -> Self {
        FileAst { symbols: Vec::new(), name_index: HashMap::new() }
    }

    fn push(&mut self, sym: Symbol) {
        let idx = self.symbols.len();
        self.name_index.entry(sym.name.clone()).or_default().push(idx);
        self.symbols.push(sym);
    }

    /// Look up symbols by name.
    pub fn find(&self, name: &str) -> Vec<&Symbol> {
        self.name_index
            .get(name)
            .map(|indices| indices.iter().map(|&i| &self.symbols[i]).collect())
            .unwrap_or_default()
    }
}

/// Per-file AST index for the entire repository.
pub type AstIndex = HashMap<String, FileAst>;

// ---------------------------------------------------------------------------
// Language resolution
// ---------------------------------------------------------------------------

/// Map a file extension to the tree-sitter Language.
fn language_for_ext(ext: &str) -> Option<Language> {
    match ext {
        "rs" => Some(tree_sitter_rust::LANGUAGE.into()),
        "ts" | "tsx" => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        "js" | "jsx" | "mjs" | "cjs" => Some(tree_sitter_javascript::LANGUAGE.into()),
        "py" | "pyi" => Some(tree_sitter_python::LANGUAGE.into()),
        "go" => Some(tree_sitter_go::LANGUAGE.into()),
        "c" | "h" => Some(tree_sitter_c::LANGUAGE.into()),
        "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => Some(tree_sitter_cpp::LANGUAGE.into()),
        "java" => Some(tree_sitter_java::LANGUAGE.into()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Symbol extraction
// ---------------------------------------------------------------------------

/// Map a tree-sitter node kind to our SymbolKind, given the file's language.
fn classify_node(kind: &str, _ext: &str) -> Option<SymbolKind> {
    match kind {
        // Rust-specific node types
        "function_item" => Some(SymbolKind::Function),
        "struct_item" => Some(SymbolKind::Struct),
        "enum_item" => Some(SymbolKind::Enum),
        "trait_item" => Some(SymbolKind::Trait),
        "impl_item" => Some(SymbolKind::Impl),
        "type_item" => Some(SymbolKind::TypeAlias),
        "const_item" => Some(SymbolKind::Constant),
        "static_item" => Some(SymbolKind::Constant),

        // JS/TS/Go/Java shared: function_declaration
        "function_declaration" => Some(SymbolKind::Function),

        // JS/TS/Java shared: class_declaration
        "class_declaration" => Some(SymbolKind::Class),

        // JS/TS/Java shared: interface_declaration
        "interface_declaration" => Some(SymbolKind::Interface),

        // JS/TS-specific
        "type_alias_declaration" => Some(SymbolKind::TypeAlias),
        "method_definition" => Some(SymbolKind::Method),
        "export_statement" => None, // descend into children

        // Python
        "function_definition" => Some(SymbolKind::Function),
        "class_definition" => Some(SymbolKind::Class),

        // Go
        "method_declaration" => Some(SymbolKind::Method),
        "type_declaration" => None, // descend into type_spec children
        "type_spec" => Some(SymbolKind::TypeAlias),

        // C / C++
        "struct_specifier" => Some(SymbolKind::Struct),
        "enum_specifier" => Some(SymbolKind::Enum),
        "class_specifier" => Some(SymbolKind::Class),

        // Java
        "enum_declaration" => Some(SymbolKind::Enum),

        _ => None,
    }
}

fn is_rust(ext: &str) -> bool {
    ext == "rs"
}
fn is_c_cpp(ext: &str) -> bool {
    matches!(ext, "c" | "h" | "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx")
}
fn is_python(ext: &str) -> bool {
    matches!(ext, "py" | "pyi")
}
fn is_go(ext: &str) -> bool {
    ext == "go"
}

/// Extract the name of a symbol node using field-name or fallback heuristics.
fn extract_name<'a>(node: &Node<'a>, source: &'a [u8], ext: &str) -> Option<String> {
    // Try standard name fields
    for field in &["name", "type"] {
        if let Some(name_node) = node.child_by_field_name(*field) {
            if let Ok(text) = name_node.utf8_text(source) {
                let name = text.trim().to_string();
                if !name.is_empty() {
                    return Some(name);
                }
            }
        }
    }

    // Rust impl blocks: extract the type name
    if node.kind() == "impl_item" && is_rust(ext) {
        // impl <Type> or impl <Trait> for <Type>
        if let Some(type_node) = node.child_by_field_name("type") {
            if let Ok(text) = type_node.utf8_text(source) {
                return Some(text.trim().to_string());
            }
        }
    }

    // Go type_spec: name is the first named child
    if node.kind() == "type_spec" && is_go(ext) {
        if let Some(child) = node.named_child(0) {
            if let Ok(text) = child.utf8_text(source) {
                return Some(text.trim().to_string());
            }
        }
    }

    // Python: function_definition and class_definition have "name" field
    // C/C++ function_definition: declarator -> identifier
    if (node.kind() == "function_definition" && is_c_cpp(ext))
        || node.kind() == "function_item"
    {
        if let Some(decl) = node.child_by_field_name("declarator") {
            // C function: int foo(...) — declarator is a function_declarator
            if let Some(name_node) = decl.child_by_field_name("declarator") {
                if let Ok(text) = name_node.utf8_text(source) {
                    return Some(text.trim().to_string());
                }
            }
            if let Ok(text) = decl.utf8_text(source) {
                // Take just the identifier part (before '(')
                let s = text.trim();
                if let Some(paren) = s.find('(') {
                    return Some(s[..paren].trim().to_string());
                }
                return Some(s.to_string());
            }
        }
    }

    None
}

/// Build a one-line signature from a node, stripping the body.
fn extract_signature(node: &Node, source: &[u8], ext: &str) -> String {
    let text = node.utf8_text(source).unwrap_or("");

    // For most languages, take the first line up to '{' or ':'
    let first_line = text.lines().next().unwrap_or("").trim();

    // Strip body opener
    let sig = if is_python(ext) {
        // Python: up to and including ':'
        if let Some(colon) = first_line.find(':') {
            &first_line[..=colon]
        } else {
            first_line
        }
    } else if let Some(brace) = first_line.find('{') {
        first_line[..brace].trim()
    } else {
        first_line
    };

    // Truncate long signatures
    if sig.len() > 200 {
        format!("{}...", &sig[..sig.floor_char_boundary(200)])
    } else {
        sig.to_string()
    }
}

/// Recursively walk a tree-sitter node and extract symbols.
fn walk_node(
    node: &Node,
    source: &[u8],
    ext: &str,
    parent_idx: Option<usize>,
    file_ast: &mut FileAst,
) {
    let kind = node.kind();

    // Check if this node is a symbol we want to extract
    if let Some(sym_kind) = classify_node(kind, ext) {
        let name = extract_name(node, source, ext).unwrap_or_default();
        let start_line = node.start_position().row + 1; // 1-based
        let end_line = node.end_position().row + 1;
        let signature = extract_signature(node, source, ext);

        // For methods inside classes/impls, fix the kind
        let final_kind = if parent_idx.is_some()
            && matches!(sym_kind, SymbolKind::Function)
            && !is_go(ext)
        {
            SymbolKind::Method
        } else {
            sym_kind
        };

        let sym = Symbol {
            name,
            kind: final_kind,
            start_line,
            end_line,
            parent_idx,
            signature,
        };
        let my_idx = file_ast.symbols.len();
        file_ast.push(sym);

        // Descend into children with this as parent (for methods in classes/impls)
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            walk_node(&child, source, ext, Some(my_idx), file_ast);
        }
    } else {
        // Not a symbol node — descend into children keeping same parent
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            walk_node(&child, source, ext, parent_idx, file_ast);
        }
    }
}

/// Parse a single file and extract its AST symbols.
/// Returns `None` if the file's language isn't supported or parsing fails.
pub fn parse_file(content: &str, ext: &str) -> Option<FileAst> {
    let lang = language_for_ext(ext)?;
    let mut parser = Parser::new();
    parser.set_language(&lang).ok()?;

    let tree = parser.parse(content, None)?;
    let root = tree.root_node();

    let mut file_ast = FileAst::new();
    let source = content.as_bytes();

    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        walk_node(&child, source, ext, None, &mut file_ast);
    }

    if file_ast.symbols.is_empty() {
        None
    } else {
        Some(file_ast)
    }
}

// ---------------------------------------------------------------------------
// Index building
// ---------------------------------------------------------------------------

/// Build an AST index for all supported files in parallel.
pub fn build_ast_index(files: &[ScannedFile]) -> AstIndex {
    let start = std::time::Instant::now();

    let results: Vec<(String, FileAst)> = files
        .par_iter()
        .filter_map(|file| {
            let content = std::fs::read_to_string(&file.abs_path).ok()?;
            let ast = parse_file(&content, &file.ext)?;
            Some((file.rel_path.clone(), ast))
        })
        .collect();

    let count = results.len();
    let total_symbols: usize = results.iter().map(|(_, ast)| ast.symbols.len()).sum();
    let index: AstIndex = results.into_iter().collect();

    debug!(
        files = count,
        symbols = total_symbols,
        time_ms = start.elapsed().as_millis() as u64,
        "AST index built"
    );

    index
}

/// Re-parse a single file and update the AST index.
pub fn update_ast_for_file(index: &mut AstIndex, rel_path: &str, abs_path: &std::path::Path, ext: &str) {
    if let Ok(content) = std::fs::read_to_string(abs_path) {
        if let Some(ast) = parse_file(&content, ext) {
            index.insert(rel_path.to_string(), ast);
        } else {
            index.remove(rel_path);
        }
    } else {
        index.remove(rel_path);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_rust_file() {
        let src = r#"
pub fn greet(name: &str) -> String {
    format!("Hello, {}!", name)
}

struct Config {
    name: String,
    verbose: bool,
}

impl Config {
    fn new() -> Self {
        Config { name: String::new(), verbose: false }
    }
}

enum Status {
    Ok,
    Error(String),
}

trait Processor {
    fn process(&self) -> bool;
}

type Result<T> = std::result::Result<T, String>;

const MAX_SIZE: usize = 1024;
"#;
        let ast = parse_file(src, "rs").expect("Should parse Rust");
        assert!(ast.symbols.len() >= 6, "Expected >=6 symbols, got {}: {:?}",
            ast.symbols.len(), ast.symbols.iter().map(|s| format!("{} {:?}", s.name, s.kind)).collect::<Vec<_>>());

        // Check specific symbols
        let fns: Vec<&Symbol> = ast.symbols.iter().filter(|s| s.kind == SymbolKind::Function).collect();
        assert!(!fns.is_empty(), "Should find at least one function");
        assert!(fns.iter().any(|s| s.name == "greet"), "Should find 'greet'");

        let structs: Vec<&Symbol> = ast.symbols.iter().filter(|s| s.kind == SymbolKind::Struct).collect();
        assert!(structs.iter().any(|s| s.name == "Config"), "Should find 'Config' struct");

        let enums: Vec<&Symbol> = ast.symbols.iter().filter(|s| s.kind == SymbolKind::Enum).collect();
        assert!(enums.iter().any(|s| s.name == "Status"), "Should find 'Status' enum");
    }

    #[test]
    fn test_parse_typescript_file() {
        let src = r#"
export function formatName(name: string): string {
    return name.trim().toLowerCase();
}

export class App {
    private config: AppConfig;

    constructor(config: AppConfig) {
        this.config = config;
    }

    getName(): string {
        return this.config.title;
    }
}

interface AppConfig {
    title: string;
    debug: boolean;
}

type Result<T> = { ok: T } | { error: string };
"#;
        let ast = parse_file(src, "ts").expect("Should parse TypeScript");
        assert!(ast.symbols.len() >= 3, "Expected >=3 symbols, got {}", ast.symbols.len());

        assert!(!ast.find("formatName").is_empty(), "Should find formatName");
        assert!(!ast.find("App").is_empty(), "Should find App class");
        assert!(!ast.find("AppConfig").is_empty(), "Should find AppConfig interface");
    }

    #[test]
    fn test_parse_python_file() {
        let src = r#"
def greet(name: str) -> str:
    return f"Hello, {name}!"

class Config:
    def __init__(self, name: str):
        self.name = name

    def process(self) -> bool:
        return len(self.name) > 0
"#;
        let ast = parse_file(src, "py").expect("Should parse Python");
        assert!(!ast.find("greet").is_empty(), "Should find greet");
        assert!(!ast.find("Config").is_empty(), "Should find Config class");
    }

    #[test]
    fn test_nested_symbols() {
        let src = r#"
impl Config {
    pub fn new() -> Self {
        Config { name: String::new(), verbose: false }
    }

    pub fn validate(&self) -> bool {
        !self.name.is_empty()
    }
}
"#;
        let ast = parse_file(src, "rs").expect("Should parse Rust impl");
        // Methods inside impl should have parent_idx set
        let methods: Vec<&Symbol> = ast.symbols.iter()
            .filter(|s| s.kind == SymbolKind::Method)
            .collect();
        assert!(!methods.is_empty(), "Should find methods in impl block");
        for m in &methods {
            assert!(m.parent_idx.is_some(), "Method '{}' should have parent_idx", m.name);
        }
    }

    #[test]
    fn test_symbol_line_ranges() {
        let src = "fn foo() {\n    1 + 1;\n}\n\nfn bar() {\n    2 + 2;\n}\n";
        let ast = parse_file(src, "rs").expect("Should parse");
        assert!(ast.symbols.len() >= 2, "Expected 2 functions");

        let foo = ast.find("foo");
        assert!(!foo.is_empty(), "Should find foo");
        assert_eq!(foo[0].start_line, 1);
        assert_eq!(foo[0].end_line, 3);

        let bar = ast.find("bar");
        assert!(!bar.is_empty(), "Should find bar");
        assert_eq!(bar[0].start_line, 5);
        assert_eq!(bar[0].end_line, 7);
    }

    #[test]
    fn test_signature_extraction() {
        let src = r#"
pub fn process(config: &Config, verbose: bool) -> Result<String, Error> {
    todo!()
}
"#;
        let ast = parse_file(src, "rs").expect("Should parse");
        let syms = ast.find("process");
        assert!(!syms.is_empty());
        let sig = &syms[0].signature;
        assert!(sig.contains("pub fn process"), "Signature should contain fn name: {sig}");
        assert!(sig.contains("Result"), "Signature should contain return type: {sig}");
        assert!(!sig.contains('{'), "Signature should not contain body: {sig}");
    }

    #[test]
    fn test_unknown_ext_returns_none() {
        let src = "some random text content";
        assert!(parse_file(src, "txt").is_none());
        assert!(parse_file(src, "md").is_none());
        assert!(parse_file(src, "toml").is_none());
    }

    #[test]
    fn test_ast_index_build() {
        // Build with empty file list should produce empty index
        let index = build_ast_index(&[]);
        assert!(index.is_empty());
    }

    #[test]
    fn test_incremental_ast_update() {
        let mut index = AstIndex::new();
        let src = "fn hello() {}\n";
        let ast = parse_file(src, "rs").unwrap();
        index.insert("src/main.rs".to_string(), ast);
        assert!(index.contains_key("src/main.rs"));

        // Remove
        index.remove("src/main.rs");
        assert!(!index.contains_key("src/main.rs"));
    }
}
