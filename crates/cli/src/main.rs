//! CodeScope CLI — command-line search and analysis tool.
//!
//! Calls `codescope-core` directly with no server overhead.

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;

use codescope_core::fuzzy::{preprocess_search_query, run_search};
use codescope_core::scan::get_category_path;
use codescope_core::types::*;
use codescope_core::{load_codescope_config, scan_repo, tokenizer};

/// CodeScope CLI — fast codebase search from the terminal.
#[derive(Parser)]
#[command(name = "cs", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Output as JSON instead of human-readable text
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Fuzzy search for files by name
    Search {
        /// Search query
        query: String,

        /// Project root (default: current directory)
        #[arg(long)]
        root: Option<PathBuf>,

        /// Filter by file extension
        #[arg(long)]
        ext: Option<String>,

        /// Maximum number of results
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Grep for content within files
    Grep {
        /// Grep pattern
        pattern: String,

        /// Project root (default: current directory)
        #[arg(long)]
        root: Option<PathBuf>,

        /// Use regex mode
        #[arg(long)]
        regex: bool,

        /// Maximum number of results
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Read a file's contents
    Read {
        /// File path (relative to project root)
        path: String,

        /// Project root (default: current directory)
        #[arg(long)]
        root: Option<PathBuf>,

        /// Start line (1-indexed)
        #[arg(long)]
        start: Option<usize>,

        /// End line (1-indexed)
        #[arg(long)]
        end: Option<usize>,
    },
    /// Show project status (indexed files, modules, etc.)
    Status {
        /// Project root (default: current directory)
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// Initialize CodeScope in a project
    Init {
        /// Project path (default: current directory)
        path: Option<PathBuf>,

        /// Add to global config
        #[arg(long)]
        global: bool,
    },
}

fn resolve_root(root: Option<PathBuf>) -> PathBuf {
    root.unwrap_or_else(|| std::env::current_dir().expect("Could not determine current directory"))
        .canonicalize()
        .expect("Path not found")
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("codescope=warn".parse().unwrap()),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Search { query, root, ext, limit } => {
            let root = resolve_root(root);
            let name = root.file_name().and_then(|n| n.to_str()).unwrap_or("project");
            let tok = tokenizer::create_tokenizer("bytes-estimate");
            let repo = scan_repo(name, &root, &tok);

            let search_query = preprocess_search_query(&query);
            let results = run_search(&repo.search_files, &repo.search_modules, &search_query, limit, 5);

            if cli.json {
                let items: Vec<serde_json::Value> = results
                    .files
                    .iter()
                    .map(|r| {
                        serde_json::json!({
                            "path": r.path,
                            "score": r.score,
                            "category": get_category_path(&r.path, &repo.config),
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&items).unwrap());
            } else {
                if results.files.is_empty() {
                    eprintln!("No results for '{query}'");
                    std::process::exit(1);
                }
                for r in &results.files {
                    let cat = get_category_path(&r.path, &repo.config);
                    println!("{:<60} {:>6.1}  {}", r.path, r.score, cat.join("/"));
                }
                eprintln!("\n{} results", results.files.len());
            }
        }
        Commands::Grep { pattern, root, regex: _regex_mode, limit } => {
            let root = resolve_root(root);
            let name = root.file_name().and_then(|n| n.to_str()).unwrap_or("project");
            let tok = tokenizer::create_tokenizer("bytes-estimate");
            let repo = scan_repo(name, &root, &tok);

            // Simple grep across all files
            let re = regex::RegexBuilder::new(&pattern)
                .case_insensitive(true)
                .build();
            let re = match re {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("Invalid pattern: {e}");
                    std::process::exit(1);
                }
            };

            let mut matches = Vec::new();
            for file in &repo.all_files {
                if let Ok(content) = std::fs::read_to_string(&file.abs_path) {
                    for (i, line) in content.lines().enumerate() {
                        if re.is_match(line) {
                            matches.push((file.rel_path.clone(), i + 1, line.to_string()));
                            if matches.len() >= limit * 5 {
                                break;
                            }
                        }
                    }
                }
                if matches.len() >= limit * 5 {
                    break;
                }
            }

            if cli.json {
                let items: Vec<serde_json::Value> = matches
                    .iter()
                    .take(limit)
                    .map(|(path, line, text)| {
                        serde_json::json!({
                            "path": path,
                            "line": line,
                            "text": text.trim(),
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&items).unwrap());
            } else {
                if matches.is_empty() {
                    eprintln!("No matches for '{pattern}'");
                    std::process::exit(1);
                }
                for (path, line, text) in matches.iter().take(limit) {
                    println!("{}:{}: {}", path, line, text.trim());
                }
                eprintln!("\n{} matches (showing {})", matches.len(), matches.len().min(limit));
            }
        }
        Commands::Read { path, root, start, end } => {
            let root = resolve_root(root);
            let full_path = root.join(&path);
            if !full_path.exists() {
                eprintln!("File not found: {path}");
                std::process::exit(1);
            }

            let content = std::fs::read_to_string(&full_path).unwrap_or_else(|e| {
                eprintln!("Could not read {path}: {e}");
                std::process::exit(1);
            });

            let lines: Vec<&str> = content.lines().collect();
            let start = start.unwrap_or(1).max(1) - 1;
            let end = end.unwrap_or(lines.len()).min(lines.len());

            if cli.json {
                let output = serde_json::json!({
                    "path": path,
                    "start_line": start + 1,
                    "end_line": end,
                    "total_lines": lines.len(),
                    "content": lines[start..end].join("\n"),
                });
                println!("{}", serde_json::to_string_pretty(&output).unwrap());
            } else {
                for (i, line) in lines[start..end].iter().enumerate() {
                    println!("{:>5} | {}", start + i + 1, line);
                }
            }
        }
        Commands::Status { root } => {
            let root = resolve_root(root);
            let name = root.file_name().and_then(|n| n.to_str()).unwrap_or("project");
            let tok = tokenizer::create_tokenizer("bytes-estimate");
            let repo = scan_repo(name, &root, &tok);

            if cli.json {
                let output = serde_json::json!({
                    "name": repo.name,
                    "root": repo.root.display().to_string(),
                    "files": repo.all_files.len(),
                    "modules": repo.manifest.len(),
                    "scan_time_ms": repo.scan_time_ms,
                });
                println!("{}", serde_json::to_string_pretty(&output).unwrap());
            } else {
                println!("Project:    {}", repo.name);
                println!("Root:       {}", repo.root.display());
                println!("Files:      {}", repo.all_files.len());
                println!("Modules:    {}", repo.manifest.len());
                println!("Scan time:  {}ms", repo.scan_time_ms);

                // Top extensions
                let mut ext_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
                for f in &repo.all_files {
                    *ext_counts.entry(&f.ext).or_default() += 1;
                }
                let mut exts: Vec<_> = ext_counts.into_iter().collect();
                exts.sort_by(|a, b| b.1.cmp(&a.1));
                println!("\nTop extensions:");
                for (ext, count) in exts.iter().take(10) {
                    println!("  .{:<12} {}", ext, count);
                }
            }
        }
        Commands::Init { path, global } => {
            let mut args = vec!["init".to_string()];
            if let Some(p) = path {
                args.push(p.display().to_string());
            }
            if global {
                args.push("--global".to_string());
            }
            std::process::exit(codescope_core::init::run_init(&args));
        }
    }
}
