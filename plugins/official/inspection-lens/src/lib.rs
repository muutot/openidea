//! Inspection Lens for OpenIDEA: render the Problems view's diagnostics
//! inline, after the text of the line they belong to.
//!
//! `host::problems` is the source (LSP, linters, spelling and plugin
//! diagnostics alike); `host::set_decorations` draws one annotation per line
//! with a severity style, a native hover tooltip and the message text after
//! the line. `Project::read_file` supplies the UTF-16 line ends (unsaved
//! editor content wins). Everything the settings page exposes is read on
//! every refresh, so a settings change takes effect without a restart.

use openidea_plugin::{
    Decoration, Diagnostic, Error, Event, Invocation, Plugin, Position, Project, Range, Severity, host, register_plugin,
};
use std::collections::BTreeMap;

const SET: &str = "inspection-lens";
const PATHS_KEY: &str = "decorated-paths";
/// Space between the code and the first message, as in Inspection Lens.
const PREFIX: &str = "  ";

/// PyCharm's three hover modes. Plugin annotations cannot bind click intents in
/// OpenIDEA, so the two click modes map to how much the tooltip carries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum HoverMode {
    Disabled,
    #[default]
    Full,
    Message,
}

/// The categories of the Shown Severities settings; each diagnostic is in one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Category {
    Agent,
    Errors,
    Warnings,
    DebuggerSuggestions,
    Information,
    WeakWarnings,
    ServerProblems,
    SyntaxErrors,
    Typos,
    StyleSuggestions,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Shown {
    agent: bool,
    errors: bool,
    warnings: bool,
    debugger_suggestions: bool,
    information: bool,
    weak_warnings: bool,
    server_problems: bool,
    syntax_errors: bool,
    typos: bool,
    style_suggestions: bool,
    other: bool,
}

impl Default for Shown {
    fn default() -> Self {
        Self {
            agent: true,
            errors: true,
            warnings: true,
            debugger_suggestions: true,
            information: true,
            weak_warnings: true,
            server_problems: true,
            syntax_errors: true,
            typos: true,
            style_suggestions: true,
            other: true,
        }
    }
}

impl Shown {
    fn get(&self, category: Category) -> bool {
        match category {
            Category::Agent => self.agent,
            Category::Errors => self.errors,
            Category::Warnings => self.warnings,
            Category::DebuggerSuggestions => self.debugger_suggestions,
            Category::Information => self.information,
            Category::WeakWarnings => self.weak_warnings,
            Category::ServerProblems => self.server_problems,
            Category::SyntaxErrors => self.syntax_errors,
            Category::Typos => self.typos,
            Category::StyleSuggestions => self.style_suggestions,
            Category::Other => self.other,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Config {
    use_editor_font: bool,
    max_description_length: usize,
    hover_mode: HoverMode,
    shown: Shown,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            use_editor_font: true,
            max_description_length: 120,
            hover_mode: HoverMode::default(),
            shown: Shown::default(),
        }
    }
}

fn bool_setting(key: &str, default: bool) -> bool {
    host::setting_bool(key).unwrap_or(default)
}

fn usize_setting(key: &str, default: usize) -> usize {
    host::setting_number(key).and_then(|n| (0.0..=10_000.0).contains(&n).then_some(n as usize)).unwrap_or(default)
}

impl Config {
    fn read() -> Self {
        let d = Self::default();
        Self {
            use_editor_font: bool_setting("useEditorFont", d.use_editor_font),
            max_description_length: usize_setting("maxDescriptionLength", d.max_description_length),
            hover_mode: match host::setting_string("hoverMode").as_deref() {
                Some("Disabled") => HoverMode::Disabled,
                Some("Left click jumps to highlight, middle click shows intentions") => HoverMode::Message,
                _ => HoverMode::Full,
            },
            shown: Shown {
                agent: bool_setting("showAgent", d.shown.agent),
                errors: bool_setting("showErrors", d.shown.errors),
                warnings: bool_setting("showWarnings", d.shown.warnings),
                debugger_suggestions: bool_setting("showDebuggerSuggestions", d.shown.debugger_suggestions),
                information: bool_setting("showInformation", d.shown.information),
                weak_warnings: bool_setting("showWeakWarnings", d.shown.weak_warnings),
                server_problems: bool_setting("showServerProblems", d.shown.server_problems),
                syntax_errors: bool_setting("showSyntaxErrors", d.shown.syntax_errors),
                typos: bool_setting("showTypos", d.shown.typos),
                style_suggestions: bool_setting("showStyleSuggestions", d.shown.style_suggestions),
                other: bool_setting("showOther", d.shown.other),
            },
        }
    }
}

/// Spelling / grammar problems are reported by several producers with no
/// shared severity, so recognise them by their message, tool or rule id.
fn is_typo(d: &Diagnostic) -> bool {
    ["typo", "spell", "grammar", "拼写"].iter().any(|k| haystack(d).contains(k))
}

fn haystack(d: &Diagnostic) -> String {
    format!(
        "{} {} {}",
        d.message.to_ascii_lowercase(),
        d.source.as_deref().unwrap_or_default().to_ascii_lowercase(),
        d.code.as_deref().unwrap_or_default().to_ascii_lowercase(),
    )
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|k| haystack.contains(k))
}

/// One category per diagnostic, in the order the specific kinds win over severity.
fn classify(d: &Diagnostic) -> Category {
    let hay = haystack(d);
    if contains_any(&hay, &["agent", "ai suggestion", "智能体"]) {
        return Category::Agent;
    }
    if is_typo(d) {
        return Category::Typos;
    }
    if d.unnecessary || d.deprecated {
        return Category::WeakWarnings;
    }
    if contains_any(&hay, &["debugger", "breakpoint"]) {
        return Category::DebuggerSuggestions;
    }
    if contains_any(&hay, &["syntax", "parse error", "unexpected token", "expected expression"]) {
        return Category::SyntaxErrors;
    }
    if contains_any(&hay, &["style", "convention", "naming", "formatting", "lint"]) {
        return Category::StyleSuggestions;
    }
    if contains_any(&hay, &["language server", "lsp", "server crashed", "server error"]) {
        return Category::ServerProblems;
    }
    match d.severity {
        Severity::Error => Category::Errors,
        Severity::Warning => Category::Warnings,
        Severity::Info => Category::Information,
        Severity::Hint => Category::Other,
    }
}

fn visible(cfg: &Config, d: &Diagnostic) -> bool {
    cfg.shown.get(classify(d))
}

fn rank(d: &Diagnostic) -> u8 {
    match d.severity {
        Severity::Error => 0,
        Severity::Warning => 1,
        Severity::Info => 2,
        Severity::Hint => 3,
    }
}

/// The decoration style, named after the Color Scheme attribute whose colour it uses
/// (Error, Warning, Weak warning, Typo, Information): editing the scheme recolours the
/// annotation together with the code it describes.
fn style(cfg: &Config, d: &Diagnostic) -> &'static str {
    if !cfg.use_editor_font {
        return "muted";
    }
    match classify(d) {
        Category::Errors | Category::SyntaxErrors | Category::ServerProblems => "error",
        Category::Warnings => "warning",
        Category::WeakWarnings => "weak",
        Category::Typos => "typo",
        Category::Information | Category::StyleSuggestions | Category::DebuggerSuggestions | Category::Agent => "info",
        Category::Other => "muted",
    }
}

/// One line of the message, truncated at the configured length.
fn message(cfg: &Config, d: &Diagnostic) -> String {
    let mut out: String = d.message.split_whitespace().collect::<Vec<_>>().join(" ");
    if cfg.max_description_length > 0 && out.chars().count() > cfg.max_description_length {
        out = out.chars().take(cfg.max_description_length.saturating_sub(1)).collect();
        out.push('…');
    }
    out
}

fn full_message(d: &Diagnostic) -> String {
    let mut out = String::new();
    if let Some(source) = &d.source {
        out.push_str(&format!("[{source}] "));
    }
    out.push_str(d.message.trim());
    if let Some(code) = &d.code {
        out.push_str(&format!(" ({code})"));
    }
    for related in &d.related {
        out.push_str(&format!("\n{}: {}", related.location.path, related.message));
    }
    out
}

/// UTF-16 column just after the line's text, falling back to the diagnostic's
/// own end when the document cannot be read (a file with no open editor).
fn line_end(line: u32, text: Option<&str>, d: &Diagnostic) -> u32 {
    text.and_then(|t| t.lines().nth(line as usize))
        .map(|l| l.encode_utf16().count() as u32)
        .unwrap_or_else(|| d.range.end.character.max(d.range.start.character))
}

/// Pure rendering: the settings and a file's diagnostics in, decorations out.
fn decorations_for(cfg: &Config, diagnostics: &[Diagnostic], text: Option<&str>) -> Vec<Decoration> {
    let mut by_line: BTreeMap<u32, Vec<&Diagnostic>> = BTreeMap::new();
    for d in diagnostics.iter().filter(|d| visible(cfg, d)) {
        by_line.entry(d.range.start.line).or_default().push(d);
    }
    let mut out = Vec::with_capacity(by_line.len());
    for (line, mut group) in by_line {
        group.sort_by_key(|d| (rank(d), d.range.start.character));
        let hidden = group.len() - 1;
        let mut after = format!("{PREFIX}{}", message(cfg, group[0]));
        if hidden > 0 {
            after.push_str(&format!(" · +{hidden} more"));
        }
        let hover = match cfg.hover_mode {
            HoverMode::Disabled => None,
            HoverMode::Message => {
                Some(group.iter().map(|d| d.message.trim().to_owned()).collect::<Vec<_>>().join("\n"))
            }
            HoverMode::Full => Some(group.iter().map(|d| full_message(d)).collect::<Vec<_>>().join("\n")),
        };
        let first = group[0];
        let end = line_end(line, text, first);
        out.push(Decoration {
            range: Range::new(Position::new(line, end), Position::new(line, end)),
            style: style(cfg, first).into(),
            hover,
            after: Some(after),
        });
    }
    out
}

fn document_text(path: &str) -> Option<String> {
    let project: Project = host::project_of(path)?;
    // `read_file` already prefers unsaved editor content, and is available
    // without a focused editor (unlike the versioned document API).
    project.read_file(path).ok()
}

fn load_paths() -> Vec<String> {
    host::storage::get(PATHS_KEY).ok().flatten().and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default()
}

fn save_paths(paths: &[String]) {
    if let Ok(json) = serde_json::to_string(paths) {
        let _ = host::storage::set(PATHS_KEY, &json);
    }
}

#[derive(Default)]
pub struct InspectionLens;

impl InspectionLens {
    /// Re-read every setting and the whole Problems snapshot. Called from every
    /// subscribed event and from the Refresh command, which both carry a window.
    fn refresh(&self) {
        let cfg = Config::read();
        // `problems` needs an invoking window. A window-less activation keeps
        // the last snapshot instead of wiping it.
        let Ok(files) = host::problems(None) else { return };
        let mut per_file: BTreeMap<String, Vec<Diagnostic>> = BTreeMap::new();
        for file in files {
            let entry = per_file.entry(file.path).or_default();
            for d in file.diagnostics {
                if !entry.iter().any(|e| e.range == d.range && e.message == d.message) {
                    entry.push(d);
                }
            }
        }
        // Files whose last diagnostic disappeared still need their set cleared.
        for path in load_paths() {
            per_file.entry(path).or_default();
        }
        let mut paths = Vec::new();
        for (path, diagnostics) in &per_file {
            let items = if diagnostics.is_empty() {
                Vec::new()
            } else {
                decorations_for(&cfg, diagnostics, document_text(path).as_deref())
            };
            if host::set_decorations(path, SET, &items).is_ok() {
                paths.push(path.clone());
            }
        }
        save_paths(&paths);
    }
}

impl Plugin for InspectionLens {
    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        match id {
            "refresh" => {
                self.refresh();
                Ok(None)
            }
            _ => Err(Error::NotFound(format!("command {id}"))),
        }
    }

    fn handle_events(&mut self, events: &[Event], _ctx: &Invocation) {
        if !events.is_empty() {
            self.refresh();
        }
    }
}

register_plugin!(InspectionLens);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    fn diag(line: u32, sev: Severity, msg: &str) -> Diagnostic {
        Diagnostic::new(Range::new(Position::new(line, 0), Position::new(line, 3)), sev, msg)
    }

    #[test]
    fn renders_at_utf16_line_end_with_the_severity_style() {
        let items = decorations_for(&Config::default(), &[diag(1, Severity::Error, "boom")], Some("abc\n😀x\n"));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].range, Range::new(Position::new(1, 3), Position::new(1, 3)));
        assert_eq!(items[0].style, "error");
        assert_eq!(items[0].after.as_deref(), Some("  boom"));
        assert!(items[0].hover.as_deref().unwrap().contains("boom"));
    }

    #[test]
    fn groups_by_line_orders_by_severity_and_collapses_extras() {
        let diags = vec![
            diag(0, Severity::Info, "third"),
            diag(0, Severity::Error, "first"),
            diag(0, Severity::Warning, "second"),
            diag(1, Severity::Hint, "other line"),
        ];
        let items = decorations_for(&Config::default(), &diags, Some("abcd\nx\n"));
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].after.as_deref(), Some("  first · +2 more"));
        assert_eq!(items[1].style, "muted");
    }

    #[test]
    fn editor_font_switch_and_hover_modes() {
        let mut d = diag(0, Severity::Error, "a rather long message").code("E42");
        d.source = Some("rust-analyzer".into());

        let items = decorations_for(&Config::default(), std::slice::from_ref(&d), Some("abcdef\n"));
        assert_eq!(items[0].style, "error");
        assert_eq!(items[0].after.as_deref(), Some("  a rather long message"));
        assert!(items[0].hover.as_deref().unwrap().starts_with("[rust-analyzer] a rather long message (E42)"));

        let cfg = Config { use_editor_font: false, ..Default::default() };
        assert_eq!(decorations_for(&cfg, std::slice::from_ref(&d), Some("abcdef\n"))[0].style, "muted");

        let cfg = Config { hover_mode: HoverMode::Message, ..Default::default() };
        assert_eq!(
            decorations_for(&cfg, std::slice::from_ref(&d), Some("abcdef\n"))[0].hover.as_deref(),
            Some("a rather long message")
        );

        let cfg = Config { hover_mode: HoverMode::Disabled, ..Default::default() };
        assert_eq!(decorations_for(&cfg, std::slice::from_ref(&d), Some("abcdef\n"))[0].hover, None);
    }

    #[test]
    fn every_category_switches_independently() {
        let mut typo = diag(0, Severity::Info, "拼写错误: 在单词 'remot' 中").code("typo");
        let mut weak = diag(1, Severity::Warning, "unused import");
        weak.unnecessary = true;
        let syntax = diag(2, Severity::Error, "invalid syntax");
        let style = diag(3, Severity::Warning, "naming convention");
        let server = diag(4, Severity::Error, "language server crashed");
        let hint = diag(5, Severity::Hint, "just a hint");
        let agent = diag(6, Severity::Info, "agent suggestion");
        let debugger = diag(7, Severity::Info, "breakpoint on a debugger pause");
        let diags = [
            typo.clone(),
            weak.clone(),
            syntax.clone(),
            style.clone(),
            server.clone(),
            hint.clone(),
            agent.clone(),
            debugger.clone(),
        ];

        assert_eq!(classify(&agent), Category::Agent);
        assert_eq!(classify(&debugger), Category::DebuggerSuggestions);
        assert_eq!(classify(&typo), Category::Typos);
        assert_eq!(classify(&weak), Category::WeakWarnings);
        assert_eq!(classify(&syntax), Category::SyntaxErrors);
        assert_eq!(classify(&style), Category::StyleSuggestions);
        assert_eq!(classify(&server), Category::ServerProblems);
        assert_eq!(classify(&hint), Category::Other);

        assert_eq!(decorations_for(&Config::default(), &diags, None).len(), 8);

        let cfg = Config {
            shown: Shown { typos: false, syntax_errors: false, other: false, ..Shown::default() },
            ..Default::default()
        };
        let items = decorations_for(&cfg, &diags, None);
        assert_eq!(items.len(), 5);
        for item in &items {
            let after = item.after.as_deref().unwrap();
            assert!(!after.contains("拼写") && !after.contains("syntax") && !after.contains("hint"), "{after}");
        }

        typo.message = "an unrelated note".into();
        typo.code = None;
        assert!(!is_typo(&typo));
    }

    #[test]
    fn max_description_length_truncates_the_message() {
        let cfg = Config { max_description_length: 4, ..Default::default() };
        let items = decorations_for(&cfg, &[diag(0, Severity::Error, "a rather long message")], Some("abcdef\n"));
        assert_eq!(items[0].after.as_deref(), Some("  a r…"));
    }

    #[test]
    fn refresh_publishes_decorations_and_clears_stale_ones() {
        let host = MockHost::install();
        let path = "/project/a.rs";
        openidea_plugin::host::publish_diagnostics(path, &[diag(0, Severity::Warning, "watch out")]).unwrap();
        let mut p = InspectionLens;
        p.run_command("refresh", &host.invocation(), None).unwrap();
        let decos = host.decorations();
        let items = &decos[&(SET.into(), path.into())];
        assert_eq!(items[0].after.as_deref(), Some("  watch out"));
        assert_eq!(items[0].style, "warning");

        openidea_plugin::host::publish_diagnostics(path, &[]).unwrap();
        p.run_command("refresh", &host.invocation(), None).unwrap();
        assert!(host.decorations()[&(SET.into(), path.into())].is_empty());
    }

    #[test]
    fn settings_are_read_live() {
        let host = MockHost::install();
        let path = "/project/a.rs";
        openidea_plugin::host::publish_diagnostics(path, &[diag(0, Severity::Error, "boom")]).unwrap();
        let mut p = InspectionLens;
        p.run_command("refresh", &host.invocation(), None).unwrap();
        assert!(!host.decorations()[&(SET.into(), path.into())].is_empty());

        host.set_setting("useEditorFont", "false");
        host.set_setting("hoverMode", "\"Disabled\"");
        p.run_command("refresh", &host.invocation(), None).unwrap();
        let items = &host.decorations()[&(SET.into(), path.into())];
        assert_eq!(items[0].style, "muted");
        assert_eq!(items[0].hover, None);
    }

    #[test]
    fn a_windowless_activation_keeps_the_last_snapshot() {
        let host = MockHost::install();
        let path = "/project/a.rs";
        openidea_plugin::host::publish_diagnostics(path, &[diag(0, Severity::Info, "note")]).unwrap();
        let mut p = InspectionLens;
        p.run_command("refresh", &host.invocation(), None).unwrap();
        assert!(!host.decorations()[&(SET.into(), path.into())].is_empty());
        // No window: `problems` is unavailable, so nothing is withdrawn.
        host.set_project_root(None);
        p.run_command("refresh", &host.invocation(), None).unwrap();
        assert!(!host.decorations()[&(SET.into(), path.into())].is_empty());
    }
}
