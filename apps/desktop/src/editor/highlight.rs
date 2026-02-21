//! Syntax highlighting via syntect — singleton initialization and HTML generation.

use std::sync::OnceLock;
use syntect::highlighting::ThemeSet;
use syntect::html::{css_for_theme_with_class_style, ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static THEME_CSS: OnceLock<String> = OnceLock::new();

fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

/// CSS for syntax highlighting classes. Generated once from base16-ocean.dark.
pub fn theme_css() -> &'static str {
    THEME_CSS.get_or_init(|| {
        let ts = ThemeSet::load_defaults();
        let theme = &ts.themes["base16-ocean.dark"];
        css_for_theme_with_class_style(theme, ClassStyle::Spaced)
            .unwrap_or_default()
    })
}

/// Generate highlighted HTML for the given code and file extension.
/// Returns one HTML string per line.
pub fn highlight_code(code: &str, extension: &str) -> Vec<String> {
    let ss = syntax_set();
    let syntax = ss
        .find_syntax_by_extension(extension)
        .unwrap_or_else(|| ss.find_syntax_plain_text());

    let mut gen = ClassedHTMLGenerator::new_with_class_style(syntax, ss, ClassStyle::Spaced);
    for line in LinesWithEndings::from(code) {
        let _ = gen.parse_html_for_line_which_includes_newline(line);
    }
    let html = gen.finalize();
    html.lines().map(|l| l.to_string()).collect()
}
