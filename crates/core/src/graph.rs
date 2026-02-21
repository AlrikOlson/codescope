//! Code graph with structural edges: call, type reference, extends/implements.
//!
//! Builds on top of the AST index (tree-sitter) and import graph to create
//! a richer dependency graph that captures function calls, type usage, and
//! inheritance relationships between symbols.

use crate::ast::{AstIndex, SymbolKind};
use crate::types::ImportGraph;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use tree_sitter::{Language, Node, Parser};
use tracing::debug;

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Import,
    Call,
    TypeRef,
    Extends,
    Implements,
}

impl EdgeKind {
    pub fn label(&self) -> &'static str {
        match self {
            EdgeKind::Import => "import",
            EdgeKind::Call => "call",
            EdgeKind::TypeRef => "type_ref",
            EdgeKind::Extends => "extends",
            EdgeKind::Implements => "implements",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "import" => Some(EdgeKind::Import),
            "call" => Some(EdgeKind::Call),
            "type_ref" => Some(EdgeKind::TypeRef),
            "extends" => Some(EdgeKind::Extends),
            "implements" => Some(EdgeKind::Implements),
            "all" => None, // sentinel: means "all kinds"
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CodeEdge {
    pub from_file: String,
    pub from_symbol: String,
    pub to_file: String,
    pub to_symbol: String,
    pub kind: EdgeKind,
}

/// Structural code graph with forward and reverse indices.
pub struct CodeGraph {
    pub edges: Vec<CodeEdge>,
    /// from_file -> edge indices
    pub by_source: HashMap<String, Vec<usize>>,
    /// to_file -> edge indices
    pub by_target: HashMap<String, Vec<usize>>,
}

impl Default for CodeGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeGraph {
    pub fn new() -> Self {
        CodeGraph {
            edges: Vec::new(),
            by_source: HashMap::new(),
            by_target: HashMap::new(),
        }
    }

    fn push(&mut self, edge: CodeEdge) {
        let idx = self.edges.len();
        self.by_source
            .entry(edge.from_file.clone())
            .or_default()
            .push(idx);
        self.by_target
            .entry(edge.to_file.clone())
            .or_default()
            .push(idx);
        self.edges.push(edge);
    }

    /// Get edges from a file, optionally filtered by kind.
    pub fn edges_from(&self, file: &str, kind: Option<EdgeKind>) -> Vec<&CodeEdge> {
        self.by_source
            .get(file)
            .map(|indices| {
                indices
                    .iter()
                    .filter_map(|&i| {
                        let e = &self.edges[i];
                        if kind.is_none() || Some(e.kind) == kind {
                            Some(e)
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get edges to a file, optionally filtered by kind.
    pub fn edges_to(&self, file: &str, kind: Option<EdgeKind>) -> Vec<&CodeEdge> {
        self.by_target
            .get(file)
            .map(|indices| {
                indices
                    .iter()
                    .filter_map(|&i| {
                        let e = &self.edges[i];
                        if kind.is_none() || Some(e.kind) == kind {
                            Some(e)
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// Global symbol index for resolution
// ---------------------------------------------------------------------------

/// A resolved symbol location.
struct SymbolLocation {
    file: String,
    name: String,
    kind: SymbolKind,
}

/// Build a name -> locations map from the AST index.
fn build_symbol_lookup(ast_index: &AstIndex) -> HashMap<String, Vec<SymbolLocation>> {
    let mut lookup: HashMap<String, Vec<SymbolLocation>> = HashMap::new();
    for (file_path, file_ast) in ast_index {
        for sym in &file_ast.symbols {
            lookup
                .entry(sym.name.clone())
                .or_default()
                .push(SymbolLocation {
                    file: file_path.clone(),
                    name: sym.name.clone(),
                    kind: sym.kind,
                });
        }
    }
    lookup
}

/// Resolve a symbol name to a target file, prioritizing:
/// 1. Same file
/// 2. Imported files
/// 3. Any file (nearest by path similarity)
fn resolve_symbol<'a>(
    name: &str,
    from_file: &str,
    imported_files: &HashSet<&str>,
    lookup: &'a HashMap<String, Vec<SymbolLocation>>,
    kind_filter: Option<SymbolKind>,
) -> Option<&'a SymbolLocation> {
    let candidates = lookup.get(name)?;

    // Filter by kind if specified
    let filtered: Vec<&SymbolLocation> = if let Some(kind) = kind_filter {
        candidates.iter().filter(|c| c.kind == kind).collect()
    } else {
        candidates.iter().collect()
    };

    if filtered.is_empty() {
        return None;
    }

    // Priority 1: same file
    if let Some(loc) = filtered.iter().find(|c| c.file == from_file) {
        return Some(loc);
    }

    // Priority 2: imported file
    if let Some(loc) = filtered.iter().find(|c| imported_files.contains(c.file.as_str())) {
        return Some(loc);
    }

    // Priority 3: closest by directory
    let from_dir = from_file.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let mut best: Option<&SymbolLocation> = None;
    let mut best_score = 0usize;
    for loc in &filtered {
        let loc_dir = loc.file.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        let score = from_dir
            .chars()
            .zip(loc_dir.chars())
            .take_while(|(a, b)| a == b)
            .count();
        if best.is_none() || score > best_score {
            best = Some(loc);
            best_score = score;
        }
    }

    best
}

// ---------------------------------------------------------------------------
// Edge extraction via tree-sitter body walking
// ---------------------------------------------------------------------------

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

/// Extract the callee name from a call expression node.
fn extract_callee_name(node: &Node, source: &[u8]) -> Option<String> {
    // Try "function" field first (Rust, JS, TS, Go, C, C++)
    let func_node = node
        .child_by_field_name("function")
        // Python uses "function" too
        .or_else(|| node.child_by_field_name("name"))
        // Java method_invocation: "name" field
        .or_else(|| node.child_by_field_name("method"))?;

    let text = func_node.utf8_text(source).ok()?.trim().to_string();

    // Handle qualified names: take the last segment
    // e.g., "self.process" -> "process", "config::load" -> "load", "app.run" -> "run"
    let name = text
        .rsplit_once("::")
        .map(|(_, n)| n)
        .or_else(|| text.rsplit_once('.').map(|(_, n)| n))
        .unwrap_or(&text);

    if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }

    Some(name.to_string())
}

/// Extract type names from type annotation nodes.
fn extract_type_names(node: &Node, source: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    collect_type_identifiers(node, source, &mut names);
    names
}

fn collect_type_identifiers(node: &Node, source: &[u8], names: &mut Vec<String>) {
    let kind = node.kind();

    // Type identifier nodes across languages
    if kind == "type_identifier"
        || kind == "identifier"
            && node
                .parent()
                .map(|p| {
                    p.kind().contains("type")
                        || p.kind() == "type_annotation"
                        || p.kind() == "return_type"
                })
                .unwrap_or(false)
    {
        if let Ok(text) = node.utf8_text(source) {
            let name = text.trim().to_string();
            // Skip primitive types and common non-type identifiers
            if !name.is_empty()
                && !is_primitive_type(&name)
                && name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
            {
                names.push(name);
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_type_identifiers(&child, source, names);
    }
}

fn is_primitive_type(name: &str) -> bool {
    matches!(
        name,
        "bool"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "f32"
            | "f64"
            | "usize"
            | "isize"
            | "str"
            | "char"
            | "String"
            | "Vec"
            | "Option"
            | "Result"
            | "Box"
            | "Arc"
            | "Rc"
            | "Self"
            | "int"
            | "float"
            | "double"
            | "void"
            | "string"
            | "number"
            | "boolean"
            | "any"
            | "never"
            | "undefined"
            | "null"
            | "object"
            | "None"
            | "True"
            | "False"
    )
}

/// Extract extends/implements from a class/struct declaration.
fn extract_inheritance(node: &Node, source: &[u8], ext: &str) -> Vec<(String, EdgeKind)> {
    let mut results = Vec::new();
    let kind = node.kind();

    match ext {
        "ts" | "tsx" | "js" | "jsx" | "java" => {
            // Look for heritage clauses: extends_clause, implements_clause
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                let ck = child.kind();
                if ck == "class_heritage" || ck == "extends_clause" || ck == "heritage" {
                    // Walk children looking for type identifiers
                    let mut inner = child.walk();
                    for grandchild in child.children(&mut inner) {
                        if grandchild.kind() == "extends_clause" {
                            if let Some(type_node) = grandchild.child_by_field_name("type") {
                                if let Ok(text) = type_node.utf8_text(source) {
                                    results.push((text.trim().to_string(), EdgeKind::Extends));
                                }
                            }
                            // Also try iterating children
                            let mut ec = grandchild.walk();
                            for ggg in grandchild.children(&mut ec) {
                                if ggg.kind() == "type_identifier" || ggg.kind() == "identifier" {
                                    if let Ok(text) = ggg.utf8_text(source) {
                                        let t = text.trim().to_string();
                                        if !t.is_empty()
                                            && !results.iter().any(|(n, _)| n == &t)
                                        {
                                            results.push((t, EdgeKind::Extends));
                                        }
                                    }
                                }
                            }
                        } else if grandchild.kind() == "implements_clause" {
                            let mut ec = grandchild.walk();
                            for ggg in grandchild.children(&mut ec) {
                                if ggg.kind() == "type_identifier" || ggg.kind() == "identifier" {
                                    if let Ok(text) = ggg.utf8_text(source) {
                                        let t = text.trim().to_string();
                                        if !t.is_empty() {
                                            results.push((t, EdgeKind::Implements));
                                        }
                                    }
                                }
                            }
                        } else if grandchild.kind() == "type_identifier"
                            || grandchild.kind() == "identifier"
                        {
                            if let Ok(text) = grandchild.utf8_text(source) {
                                let t = text.trim().to_string();
                                if !t.is_empty() && !results.iter().any(|(n, _)| n == &t) {
                                    results.push((t, EdgeKind::Extends));
                                }
                            }
                        }
                    }
                }
            }
        }
        "py" | "pyi" => {
            // Python: class Foo(Bar, Baz): -> argument_list contains base classes
            if kind == "class_definition" {
                if let Some(args) = node.child_by_field_name("superclasses") {
                    let mut cursor = args.walk();
                    for child in args.children(&mut cursor) {
                        if child.kind() == "identifier" || child.kind() == "attribute" {
                            if let Ok(text) = child.utf8_text(source) {
                                let name = text
                                    .rsplit_once('.')
                                    .map(|(_, n)| n)
                                    .unwrap_or(text)
                                    .trim()
                                    .to_string();
                                if !name.is_empty() && name != "object" {
                                    results.push((name, EdgeKind::Extends));
                                }
                            }
                        }
                    }
                }
            }
        }
        "rs" => {
            // Rust: impl Trait for Type -> extract trait name
            if kind == "impl_item" {
                if let Some(trait_node) = node.child_by_field_name("trait") {
                    if let Ok(text) = trait_node.utf8_text(source) {
                        results.push((text.trim().to_string(), EdgeKind::Implements));
                    }
                }
            }
        }
        _ => {}
    }

    results
}

/// Walk a function body collecting call expression callee names.
fn collect_calls(node: &Node, source: &[u8], calls: &mut Vec<String>) {
    let kind = node.kind();

    if kind == "call_expression" || kind == "call" || kind == "method_invocation" {
        if let Some(name) = extract_callee_name(node, source) {
            calls.push(name);
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_calls(&child, source, calls);
    }
}

// ---------------------------------------------------------------------------
// Graph building
// ---------------------------------------------------------------------------

/// Build the code graph from AST index and import graph.
pub fn build_code_graph(
    ast_index: &AstIndex,
    import_graph: &ImportGraph,
    files: &[(String, String)], // (rel_path, abs_path)
) -> CodeGraph {
    let start = std::time::Instant::now();
    let mut graph = CodeGraph::new();

    let symbol_lookup = build_symbol_lookup(ast_index);

    for (rel_path, abs_path) in files {
        let ext = rel_path
            .rsplit_once('.')
            .map(|(_, e)| e)
            .unwrap_or("");

        let lang = match language_for_ext(ext) {
            Some(l) => l,
            None => continue,
        };

        let content = match std::fs::read_to_string(abs_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let file_ast = match ast_index.get(rel_path.as_str()) {
            Some(a) => a,
            None => continue,
        };

        // Build imported files set for resolution priority
        let imported_files: HashSet<&str> = import_graph
            .imports
            .get(rel_path.as_str())
            .map(|v| v.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();

        let mut parser = Parser::new();
        if parser.set_language(&lang).is_err() {
            continue;
        }
        let tree = match parser.parse(&content, None) {
            Some(t) => t,
            None => continue,
        };

        let source = content.as_bytes();

        // For each symbol, extract edges
        for sym in &file_ast.symbols {
            // Extract inheritance edges from class/struct/impl declarations
            if matches!(
                sym.kind,
                SymbolKind::Class
                    | SymbolKind::Struct
                    | SymbolKind::Impl
                    | SymbolKind::Interface
            ) {
                // Find the tree-sitter node for this symbol by line range
                if let Some(node) = find_node_at_line(&tree.root_node(), sym.start_line) {
                    let inheritance = extract_inheritance(&node, source, ext);
                    for (parent_name, edge_kind) in inheritance {
                        let type_kinds = [
                            SymbolKind::Class,
                            SymbolKind::Struct,
                            SymbolKind::Trait,
                            SymbolKind::Interface,
                        ];
                        // Try each type kind for resolution
                        for tk in &type_kinds {
                            if let Some(target) = resolve_symbol(
                                &parent_name,
                                rel_path,
                                &imported_files,
                                &symbol_lookup,
                                Some(*tk),
                            ) {
                                if target.file != *rel_path || target.name != sym.name {
                                    graph.push(CodeEdge {
                                        from_file: rel_path.clone(),
                                        from_symbol: sym.name.clone(),
                                        to_file: target.file.clone(),
                                        to_symbol: target.name.clone(),
                                        kind: edge_kind,
                                    });
                                    break;
                                }
                            }
                        }
                    }
                }
            }

            // Extract call edges and type references from function/method bodies
            if matches!(
                sym.kind,
                SymbolKind::Function | SymbolKind::Method
            ) {
                if let Some(node) = find_node_at_line(&tree.root_node(), sym.start_line) {
                    // Collect calls
                    let mut calls = Vec::new();
                    if let Some(body) = node.child_by_field_name("body") {
                        collect_calls(&body, source, &mut calls);
                    } else {
                        // Some languages don't use "body" field name
                        collect_calls(&node, source, &mut calls);
                    }

                    let mut seen_calls = HashSet::new();
                    for callee_name in calls {
                        if callee_name == sym.name || !seen_calls.insert(callee_name.clone()) {
                            continue; // skip self-calls and duplicates
                        }
                        let callable_kinds = [SymbolKind::Function, SymbolKind::Method];
                        for ck in &callable_kinds {
                            if let Some(target) = resolve_symbol(
                                &callee_name,
                                rel_path,
                                &imported_files,
                                &symbol_lookup,
                                Some(*ck),
                            ) {
                                graph.push(CodeEdge {
                                    from_file: rel_path.clone(),
                                    from_symbol: sym.name.clone(),
                                    to_file: target.file.clone(),
                                    to_symbol: target.name.clone(),
                                    kind: EdgeKind::Call,
                                });
                                break;
                            }
                        }
                    }

                    // Collect type references from parameters and return types
                    let type_names = extract_type_names(&node, source);
                    let mut seen_types = HashSet::new();
                    for type_name in type_names {
                        if !seen_types.insert(type_name.clone()) {
                            continue;
                        }
                        let type_kinds = [
                            SymbolKind::Struct,
                            SymbolKind::Class,
                            SymbolKind::Enum,
                            SymbolKind::Interface,
                            SymbolKind::TypeAlias,
                            SymbolKind::Trait,
                        ];
                        for tk in &type_kinds {
                            if let Some(target) = resolve_symbol(
                                &type_name,
                                rel_path,
                                &imported_files,
                                &symbol_lookup,
                                Some(*tk),
                            ) {
                                if target.file != *rel_path || target.name != sym.name {
                                    graph.push(CodeEdge {
                                        from_file: rel_path.clone(),
                                        from_symbol: sym.name.clone(),
                                        to_file: target.file.clone(),
                                        to_symbol: target.name.clone(),
                                        kind: EdgeKind::TypeRef,
                                    });
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    debug!(
        edges = graph.edges.len(),
        source_files = graph.by_source.len(),
        target_files = graph.by_target.len(),
        time_ms = start.elapsed().as_millis() as u64,
        "Code graph built"
    );

    graph
}

/// Find a tree-sitter node that starts at the given 1-based line.
fn find_node_at_line<'a>(root: &Node<'a>, target_line: usize) -> Option<Node<'a>> {
    let target_row = target_line.saturating_sub(1); // Convert to 0-based
    find_deepest_at_row(root, target_row)
}

fn find_deepest_at_row<'a>(node: &Node<'a>, target_row: usize) -> Option<Node<'a>> {
    if node.start_position().row != target_row {
        // Check children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.start_position().row <= target_row && child.end_position().row >= target_row {
                if let Some(found) = find_deepest_at_row(&child, target_row) {
                    return Some(found);
                }
            }
        }
        return None;
    }

    // This node starts at the target row. Return the first named child
    // that also starts at this row if it's a more specific match,
    // otherwise return this node.
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.start_position().row == target_row && child.is_named() {
            // Prefer the child if it's a declaration/definition type
            let ck = child.kind();
            if ck.contains("function")
                || ck.contains("class")
                || ck.contains("struct")
                || ck.contains("impl")
                || ck.contains("trait")
                || ck.contains("enum")
                || ck.contains("method")
                || ck.contains("type")
            {
                return Some(child);
            }
        }
    }

    Some(*node)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast;
    use std::collections::BTreeMap;

    fn make_import_graph(edges: &[(&str, &str)]) -> ImportGraph {
        let mut imports: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut imported_by: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (from, to) in edges {
            imports
                .entry(from.to_string())
                .or_default()
                .push(to.to_string());
            imported_by
                .entry(to.to_string())
                .or_default()
                .push(from.to_string());
        }
        ImportGraph {
            imports,
            imported_by,
        }
    }

    #[test]
    fn test_call_graph_same_file() {
        let src = r#"
fn helper() -> i32 {
    42
}

fn main() {
    let x = helper();
    println!("{}", x);
}
"#;
        let file_ast = ast::parse_file(src, "rs").unwrap();
        let mut ast_index = AstIndex::new();
        ast_index.insert("src/main.rs".to_string(), file_ast);

        let import_graph = make_import_graph(&[]);
        let files = vec![("src/main.rs".to_string(), "/dev/null".to_string())];

        // We need actual file content for tree-sitter parsing, so write to temp
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("src/main.rs");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, src).unwrap();

        let files = vec![(
            "src/main.rs".to_string(),
            file_path.to_string_lossy().to_string(),
        )];

        let graph = build_code_graph(&ast_index, &import_graph, &files);

        let call_edges: Vec<&CodeEdge> = graph
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Call)
            .collect();

        // main should call helper
        assert!(
            call_edges
                .iter()
                .any(|e| e.from_symbol == "main" && e.to_symbol == "helper"),
            "Expected main->helper call edge. Found: {:?}",
            call_edges
        );
    }

    #[test]
    fn test_call_graph_cross_file() {
        let main_src = r#"
fn main() {
    run();
}
"#;
        let app_src = r#"
pub fn run() {
    println!("running");
}
"#;
        let main_ast = ast::parse_file(main_src, "rs").unwrap();
        let app_ast = ast::parse_file(app_src, "rs").unwrap();

        let mut ast_index = AstIndex::new();
        ast_index.insert("src/main.rs".to_string(), main_ast);
        ast_index.insert("src/app.rs".to_string(), app_ast);

        let import_graph = make_import_graph(&[("src/main.rs", "src/app.rs")]);

        let dir = tempfile::tempdir().unwrap();
        let main_path = dir.path().join("src/main.rs");
        let app_path = dir.path().join("src/app.rs");
        std::fs::create_dir_all(main_path.parent().unwrap()).unwrap();
        std::fs::write(&main_path, main_src).unwrap();
        std::fs::write(&app_path, app_src).unwrap();

        let files = vec![
            (
                "src/main.rs".to_string(),
                main_path.to_string_lossy().to_string(),
            ),
            (
                "src/app.rs".to_string(),
                app_path.to_string_lossy().to_string(),
            ),
        ];

        let graph = build_code_graph(&ast_index, &import_graph, &files);

        let call_edges: Vec<&CodeEdge> = graph
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Call)
            .collect();

        assert!(
            call_edges
                .iter()
                .any(|e| e.from_file == "src/main.rs"
                    && e.from_symbol == "main"
                    && e.to_file == "src/app.rs"
                    && e.to_symbol == "run"),
            "Expected cross-file main->run call edge. Found: {:?}",
            call_edges
        );
    }

    #[test]
    fn test_type_ref_edges() {
        let types_src = r#"
pub struct Config {
    pub name: String,
}
"#;
        let handler_src = r#"
fn process(cfg: Config) -> bool {
    !cfg.name.is_empty()
}
"#;
        let types_ast = ast::parse_file(types_src, "rs").unwrap();
        let handler_ast = ast::parse_file(handler_src, "rs").unwrap();

        let mut ast_index = AstIndex::new();
        ast_index.insert("src/types.rs".to_string(), types_ast);
        ast_index.insert("src/handler.rs".to_string(), handler_ast);

        let import_graph = make_import_graph(&[("src/handler.rs", "src/types.rs")]);

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/types.rs"), types_src).unwrap();
        std::fs::write(dir.path().join("src/handler.rs"), handler_src).unwrap();

        let files = vec![
            (
                "src/types.rs".to_string(),
                dir.path().join("src/types.rs").to_string_lossy().to_string(),
            ),
            (
                "src/handler.rs".to_string(),
                dir.path()
                    .join("src/handler.rs")
                    .to_string_lossy()
                    .to_string(),
            ),
        ];

        let graph = build_code_graph(&ast_index, &import_graph, &files);

        let type_edges: Vec<&CodeEdge> = graph
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::TypeRef)
            .collect();

        assert!(
            type_edges
                .iter()
                .any(|e| e.from_file == "src/handler.rs" && e.to_symbol == "Config"),
            "Expected type ref to Config. Found: {:?}",
            type_edges
        );
    }

    #[test]
    fn test_extends_edges() {
        let src = r#"
export class Animal {
    name: string;
    constructor(name: string) {
        this.name = name;
    }
}

export class Dog extends Animal {
    bark(): string {
        return "woof";
    }
}
"#;
        let ast = ast::parse_file(src, "ts").unwrap();
        let mut ast_index = AstIndex::new();
        ast_index.insert("src/animals.ts".to_string(), ast);

        let import_graph = make_import_graph(&[]);
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/animals.ts"), src).unwrap();

        let files = vec![(
            "src/animals.ts".to_string(),
            dir.path()
                .join("src/animals.ts")
                .to_string_lossy()
                .to_string(),
        )];

        let graph = build_code_graph(&ast_index, &import_graph, &files);

        let extends_edges: Vec<&CodeEdge> = graph
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Extends)
            .collect();

        assert!(
            extends_edges
                .iter()
                .any(|e| e.from_symbol == "Dog" && e.to_symbol == "Animal"),
            "Expected Dog extends Animal. Found: {:?}",
            extends_edges
        );
    }

    #[test]
    fn test_graph_edge_queries() {
        let mut graph = CodeGraph::new();
        graph.push(CodeEdge {
            from_file: "a.rs".to_string(),
            from_symbol: "main".to_string(),
            to_file: "b.rs".to_string(),
            to_symbol: "run".to_string(),
            kind: EdgeKind::Call,
        });
        graph.push(CodeEdge {
            from_file: "a.rs".to_string(),
            from_symbol: "main".to_string(),
            to_file: "c.rs".to_string(),
            to_symbol: "Config".to_string(),
            kind: EdgeKind::TypeRef,
        });

        let all_from_a = graph.edges_from("a.rs", None);
        assert_eq!(all_from_a.len(), 2);

        let calls_from_a = graph.edges_from("a.rs", Some(EdgeKind::Call));
        assert_eq!(calls_from_a.len(), 1);
        assert_eq!(calls_from_a[0].to_symbol, "run");

        let edges_to_b = graph.edges_to("b.rs", None);
        assert_eq!(edges_to_b.len(), 1);

        let edges_to_d = graph.edges_to("d.rs", None);
        assert!(edges_to_d.is_empty());
    }
}
