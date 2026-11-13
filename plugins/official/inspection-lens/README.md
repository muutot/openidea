# Inspection Lens — inline diagnostics

Renders the Problems view's diagnostics (LSP, linters, spelling checks and
other plugins' reports) as text after the line they belong to, coloured by
severity, with a native hover tooltip. Equivalent to Inspection Lens for
JetBrains IDEs / Error Lens for VS Code, built entirely on the released plugin
API 1.4:

- `host::problems(None)` — the window's Problems snapshot, grouped by source.
- `host::project_of(path)` + `Project::read_file(path)` — the file's UTF-16
  line ends, so an annotation lands after the code (unsaved editor text wins).
- `host::set_decorations(path, "inspection-lens", items)` — one annotation per
  line; `after` is the message, `style` is a fixed theme token (`error`,
  `warning`, `info`, `muted`, `underline`), `hover` is the native tooltip.
- `[contributes.settings]` with a single `schema` — the page under Settings ›
  Plugins › Inspection Lens, laid out like PyCharm's Inspection Lens page. Every
  field is read on each refresh, so changes apply immediately.
- `settings-changed`, `file-opened`, `file-saved`, `editor-focused` and
  `file-closed` events, plus the _Inspection Lens: Refresh_ command.

## Settings

The page mirrors PyCharm's Inspection Lens: Appearance / Behavior / Shown
Severities, in that order.

| Group            | Setting                                                                                                                                              | Effect                                                                                                                                                                                                    |
| ---------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Appearance       | Use editor font                                                                                                                                      | Severity colours on; off draws every message muted.                                                                                                                                                       |
| Appearance       | Max description length                                                                                                                               | Truncate long messages with an ellipsis (0 = unlimited).                                                                                                                                                  |
| Behavior         | Hover mode                                                                                                                                           | `Disabled`, or one of PyCharm's two click modes: the first shows the tooltip with tool, rule id and related locations, the second the message alone.                                                      |
| Shown Severities | Agent / Errors / Warnings / Debugger suggestions / Information / Weak warnings / Server problems / Syntax errors / Typos / Style suggestions / Other | One category per diagnostic. Weak warnings are `unnecessary` / `deprecated`; typos, syntax, style, server and debugger problems are recognised by their message, tool or rule id; hints fall under Other. |

The inline text takes its colour from the Color Scheme attributes of the same
kind — Error and Warning effect colours, Weak warning and Typo — so
Settings › Editor › Color Scheme › General recolours both the code and its
annotation. Information messages use the IDE's information colour, and Other
uses the muted text colour.

Every category header shows an example message, because the host settings page
has no preview widget of its own.

The host's decoration API has no click binding and no font control, so:

- the two click modes of Hover mode only change how much the native tooltip
  carries (the plugin cannot bind left/middle click);
- _Use editor font_ switches between severity colours and muted text, not the
  font itself (annotations always use the editor font);
- PyCharm's _Reset to Default_ action has no counterpart: plugins cannot write
  their own settings, so the page's Reset discards staged edits only.
- _Agent_ and _Debugger suggestions_ are kept for parity; OpenIDEA reports no
  such problems yet.

## Limitations

- `problems` needs an invoking window, and there is no `diagnostics-changed`
  event in the API. Refresh happens on the events above, when settings change,
  and via _Inspection Lens: Refresh_ (Search Everywhere). Diagnostics arriving
  while the editor is completely idle appear on the next refresh. Typing alone
  does not refresh (that would need the API 1.6 `document-changed` subscription
  plus the `editor.read` grant); save, switch files or run _Refresh_ instead.
- Decoration styles are fixed theme tokens; the plugin cannot define its own
  colours or fonts.
- Remote projects currently report an empty Problems snapshot to plugins.

## Build & verify

```sh
plugins/build.sh official/inspection-lens
cargo test --manifest-path plugins/official/inspection-lens/Cargo.toml
cargo run -p ide-host --bin openidea-plugin -- check plugins/official/inspection-lens
```

Install the built package folder through Settings › Plugins › Install from
Disk, then open a file with a problem and run _Inspection Lens: Refresh_ (the
page appears under Settings › Plugins › Inspection Lens as one level, with no
nested folder). Grants: `project.read`, `events`, `storage` (all low-risk).
