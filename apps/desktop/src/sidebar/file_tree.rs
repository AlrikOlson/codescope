//! File tree component — navigable directory tree from scanned files.

use std::collections::BTreeMap;
use dioxus::prelude::*;
use crate::state::*;

/// A node in the file tree.
#[derive(Clone, PartialEq)]
enum TreeNode {
    Dir {
        name: String,
        children: BTreeMap<String, TreeNode>,
    },
    File {
        name: String,
        rel_path: String,
    },
}

/// Build a tree from a list of relative file paths.
fn build_tree(paths: &[String]) -> BTreeMap<String, TreeNode> {
    let mut root: BTreeMap<String, TreeNode> = BTreeMap::new();

    for path in paths {
        let parts: Vec<&str> = path.split('/').collect();
        insert_path(&mut root, &parts, path);
    }
    root
}

fn insert_path(node: &mut BTreeMap<String, TreeNode>, parts: &[&str], full_path: &str) {
    if parts.is_empty() {
        return;
    }
    if parts.len() == 1 {
        node.insert(
            parts[0].to_string(),
            TreeNode::File {
                name: parts[0].to_string(),
                rel_path: full_path.to_string(),
            },
        );
        return;
    }
    let dir_name = parts[0].to_string();
    let entry = node.entry(dir_name.clone()).or_insert_with(|| TreeNode::Dir {
        name: dir_name,
        children: BTreeMap::new(),
    });
    if let TreeNode::Dir { children, .. } = entry {
        insert_path(children, &parts[1..], full_path);
    }
}

#[component]
pub fn FileTree() -> Element {
    let tree = use_memo(|| {
        let core = CORE.read();
        match core.as_ref() {
            Some(state) => {
                let repo = state.default_repo();
                let paths: Vec<String> = repo
                    .all_files
                    .iter()
                    .map(|f| f.rel_path.replace('\\', "/"))
                    .collect();
                build_tree(&paths)
            }
            None => BTreeMap::new(),
        }
    });

    let tree_ref = tree.read();

    rsx! {
        div {
            class: "file-tree",
            div { class: "file-tree-header", "EXPLORER" }
            div {
                class: "file-tree-content",
                for (_name, node) in tree_ref.iter() {
                    {render_node(node, "", 0)}
                }
            }
        }
    }
}

/// Render a tree node recursively.
fn render_node(node: &TreeNode, prefix: &str, depth: u32) -> Element {
    match node {
        TreeNode::Dir { name, children } => {
            let dir_path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let is_expanded = EXPANDED_DIRS.read().contains(&dir_path);
            let arrow = if is_expanded { "\u{25BE}" } else { "\u{25B8}" };
            let pad = depth * 16 + 8;

            rsx! {
                div {
                    class: "tree-dir",
                    div {
                        class: "tree-item tree-dir-label",
                        style: "padding-left: {pad}px;",
                        onclick: {
                            let dp = dir_path.clone();
                            move |_| {
                                let mut dirs = EXPANDED_DIRS.write();
                                if dirs.contains(&dp) {
                                    dirs.remove(&dp);
                                } else {
                                    dirs.insert(dp.clone());
                                }
                            }
                        },
                        span { class: "tree-arrow", "{arrow}" }
                        span { class: "tree-dir-name", "{name}" }
                    }
                    if is_expanded {
                        for (_child_name, child_node) in children.iter() {
                            {render_node(child_node, &dir_path, depth + 1)}
                        }
                    }
                }
            }
        }
        TreeNode::File { name, rel_path } => {
            let is_active = SELECTED_PATH.read().as_deref() == Some(rel_path.as_str());
            let pad = depth * 16 + 20; // extra indent for files (no arrow)

            rsx! {
                div {
                    class: if is_active { "tree-item tree-file active" } else { "tree-item tree-file" },
                    style: "padding-left: {pad}px;",
                    onclick: {
                        let p = rel_path.clone();
                        move |_| {
                            *SELECTED_PATH.write() = Some(p.clone());
                        }
                    },
                    span { class: "tree-file-name", "{name}" }
                }
            }
        }
    }
}
