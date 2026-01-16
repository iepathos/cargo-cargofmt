use std::borrow::Cow;

use crate::config::options::IndentStyle;
use crate::toml::TokenKind;
use crate::toml::TomlToken;

#[tracing::instrument]
pub fn normalize_indent(
    tokens: &mut crate::toml::TomlTokens<'_>,
    hard_tabs: bool,
    tab_spaces: usize,
    indent_style: IndentStyle,
) {
    match indent_style {
        IndentStyle::Block => normalize_indent_block(tokens, hard_tabs, tab_spaces),
        IndentStyle::Visual => normalize_indent_visual(tokens, tab_spaces),
    }
}

fn normalize_indent_block(
    tokens: &mut crate::toml::TomlTokens<'_>,
    hard_tabs: bool,
    tab_spaces: usize,
) {
    let mut depth = 0;
    let mut indices = crate::toml::TokenIndices::new();
    let mut buffer = PaddingBuffer::new(hard_tabs, tab_spaces);

    while let Some(i) = indices.next_index(tokens) {
        match tokens.tokens[i].kind {
            TokenKind::StdTableOpen | TokenKind::ArrayTableOpen => {}
            TokenKind::StdTableClose | TokenKind::ArrayTableClose => {}
            TokenKind::ArrayOpen | TokenKind::InlineTableOpen => {
                depth += 1;
            }
            TokenKind::ArrayClose | TokenKind::InlineTableClose => {
                depth -= 1;
            }
            TokenKind::SimpleKey => {}
            TokenKind::KeySep => {}
            TokenKind::KeyValSep => {}
            TokenKind::Scalar => {}
            TokenKind::ValueSep => {}
            TokenKind::Whitespace => {}
            TokenKind::Comment => {}
            TokenKind::Newline => {
                apply_block_indent(tokens, i + 1, depth, &mut buffer);
            }
            TokenKind::Error => {}
        }
    }
    tokens.trim_empty_whitespace();
}

fn apply_block_indent(
    tokens: &mut crate::toml::TomlTokens<'_>,
    next_i: usize,
    depth: usize,
    buffer: &mut PaddingBuffer,
) {
    let Some(next) = tokens.tokens.get(next_i) else {
        return;
    };

    match (next.kind, depth) {
        (TokenKind::Newline, _) | (_, 0) if next.kind != TokenKind::Whitespace => {}
        (TokenKind::Whitespace, 0) => {
            tokens.tokens[next_i] = TomlToken::EMPTY;
        }
        (TokenKind::Whitespace, _) => {
            let indent_depth = depth - close_count(tokens, next_i);
            let ws = buffer.whitespace(indent_depth);
            tokens.tokens[next_i] = make_whitespace_token(ws);
        }
        (_, _) => {
            let indent_depth = depth - close_count(tokens, next_i);
            let ws = buffer.whitespace(indent_depth);
            tokens.tokens.insert(next_i, make_whitespace_token(ws));
        }
    }
}

fn make_whitespace_token(ws: &str) -> TomlToken<'static> {
    let mut token = TomlToken::EMPTY;
    token.raw = Cow::Owned(ws.to_owned());
    token
}

/// Visual style aligns content with the opening delimiter position.
/// Always uses spaces for alignment regardless of `hard_tabs` setting.
/// Matches rustfmt behavior:
/// - First element stays on same line as opener
/// - Closing bracket on same line as last element (when no trailing comma)
fn normalize_indent_visual(tokens: &mut crate::toml::TomlTokens<'_>, tab_spaces: usize) {
    // First pass: collapse newlines after openers and before closers (rustfmt Visual style)
    collapse_leading_newlines(tokens);
    collapse_trailing_newlines(tokens);

    // Second pass: apply visual indentation to remaining newlines
    let mut column: usize = 0;
    let mut opening_columns: Vec<usize> = Vec::new();
    let mut indices = crate::toml::TokenIndices::new();
    let mut buffer = PaddingBuffer::new(false, 1); // Visual always uses spaces

    while let Some(i) = indices.next_index(tokens) {
        let token = &tokens.tokens[i];
        match token.kind {
            TokenKind::ArrayOpen | TokenKind::InlineTableOpen => {
                column += token.raw.len();
                opening_columns.push(column);
            }
            TokenKind::ArrayClose | TokenKind::InlineTableClose => {
                column += token.raw.len();
                opening_columns.pop();
            }
            TokenKind::Whitespace => {
                column += calculate_column_width(&token.raw, column, tab_spaces);
            }
            TokenKind::Newline => {
                column = 0;
                let next_i = i + 1;
                let effective_column = calculate_visual_indent(&opening_columns, tokens, next_i);
                apply_visual_indent(tokens, next_i, effective_column, &mut buffer);
            }
            _ => {
                column += token.raw.len();
            }
        }
    }
    tokens.trim_empty_whitespace();
}

/// Removes newlines (and following whitespace) immediately after array/inline table openers.
/// This brings the first element onto the same line as the opener (rustfmt Visual style).
fn collapse_leading_newlines(tokens: &mut crate::toml::TomlTokens<'_>) {
    let mut i = 0;
    while i < tokens.tokens.len() {
        if matches!(
            tokens.tokens[i].kind,
            TokenKind::ArrayOpen | TokenKind::InlineTableOpen
        ) {
            // Look ahead: remove newline and whitespace after opener
            let mut j = i + 1;
            while j < tokens.tokens.len() {
                match tokens.tokens[j].kind {
                    TokenKind::Newline | TokenKind::Whitespace => {
                        tokens.tokens[j] = TomlToken::EMPTY;
                        j += 1;
                    }
                    _ => break,
                }
            }
        }
        i += 1;
    }
}

/// Removes newlines (and preceding whitespace) immediately before array/inline table closers.
/// This puts the closer on the same line as the last element (rustfmt Visual style).
fn collapse_trailing_newlines(tokens: &mut crate::toml::TomlTokens<'_>) {
    for i in 0..tokens.tokens.len() {
        let is_closer = matches!(
            tokens.tokens[i].kind,
            TokenKind::ArrayClose | TokenKind::InlineTableClose
        );
        if is_closer {
            if let Some(collapse_start) = find_collapsible_newline(tokens, i) {
                for k in collapse_start..i {
                    tokens.tokens[k] = TomlToken::EMPTY;
                }
            }
        }
    }
}

/// Finds the start index of a collapsible sequence before a closer.
/// Returns Some(index) if there's a newline before the closer, None otherwise.
/// In Visual style, also removes trailing commas when collapsing (matches rustfmt).
fn find_collapsible_newline(
    tokens: &crate::toml::TomlTokens<'_>,
    closer_i: usize,
) -> Option<usize> {
    let mut j = closer_i.saturating_sub(1);
    let mut found_newline = false;

    while j > 0 {
        match tokens.tokens[j].kind {
            TokenKind::Newline => {
                found_newline = true;
                j = j.saturating_sub(1);
            }
            TokenKind::Whitespace | TokenKind::ValueSep => {
                j = j.saturating_sub(1);
            }
            _ => break,
        }
    }

    // Return position after the content (j+1) to include ValueSep in collapse range
    if found_newline {
        Some(j + 1)
    } else {
        None
    }
}

fn calculate_visual_indent(
    opening_columns: &[usize],
    tokens: &crate::toml::TomlTokens<'_>,
    next_i: usize,
) -> usize {
    let closes = close_count(tokens, next_i);

    if closes > 0 {
        // Align with the position OF the innermost bracket being closed (first closer on line).
        // opening_columns stores positions AFTER the opener, so subtract 1.
        // Multiple closers on the same line (e.g., `]]`) will be adjacent after the first.
        opening_columns
            .last()
            .map(|&col| col.saturating_sub(1))
            .unwrap_or(0)
    } else {
        // Align content with current nesting level (position after the opener)
        opening_columns.last().copied().unwrap_or(0)
    }
}

fn apply_visual_indent(
    tokens: &mut crate::toml::TomlTokens<'_>,
    next_i: usize,
    column: usize,
    buffer: &mut PaddingBuffer,
) {
    let Some(next) = tokens.tokens.get(next_i) else {
        return;
    };

    let ws = buffer.whitespace(column);

    match (next.kind, column) {
        (TokenKind::Newline, _) | (_, 0) if next.kind != TokenKind::Whitespace => {}
        (TokenKind::Whitespace, 0) => {
            tokens.tokens[next_i] = TomlToken::EMPTY;
        }
        (TokenKind::Whitespace, _) => {
            tokens.tokens[next_i] = make_whitespace_token(ws);
        }
        (_, _) => {
            tokens.tokens.insert(next_i, make_whitespace_token(ws));
        }
    }
}

/// Calculates the visual column width of whitespace, handling tabs
fn calculate_column_width(raw: &str, current_column: usize, tab_spaces: usize) -> usize {
    raw.chars()
        .fold((0, current_column), |(width, col), c| {
            if c == '\t' {
                let spaces_to_next_tab = tab_spaces - (col % tab_spaces);
                (width + spaces_to_next_tab, col + spaces_to_next_tab)
            } else {
                (width + 1, col + 1)
            }
        })
        .0
}

struct PaddingBuffer {
    buffer: String,
    c: &'static str,
    count_per_indent: usize,
}

impl PaddingBuffer {
    fn new(hard_tabs: bool, tab_spaces: usize) -> Self {
        let (count_per_indent, c) = if hard_tabs {
            (1, "\t")
        } else {
            (tab_spaces, " ")
        };
        Self {
            buffer: Default::default(),
            c,
            count_per_indent,
        }
    }

    fn whitespace(&mut self, depth: usize) -> &str {
        let count = depth * self.count_per_indent;

        self.buffer.truncate(count);
        if let Some(add) = count.checked_sub(self.buffer.len()) {
            self.buffer.reserve(add);
            for _ in 0..add {
                self.buffer.push_str(self.c);
            }
        }

        &self.buffer
    }
}

/// Counts closing brackets at the start of a line (before any content).
/// Used to determine alignment for lines that begin with closers like `]` or `]]`.
fn close_count(tokens: &crate::toml::TomlTokens<'_>, i: usize) -> usize {
    if i >= tokens.tokens.len() {
        return 0;
    }

    // Only count closers that appear before any content (after optional whitespace).
    // This prevents counting closers that follow content on the same line (e.g., `"sit"]`).
    tokens.tokens[i..]
        .iter()
        .take_while(|t| {
            matches!(
                t.kind,
                TokenKind::Whitespace | TokenKind::ArrayClose | TokenKind::InlineTableClose
            )
        })
        .filter(|t| matches!(t.kind, TokenKind::ArrayClose | TokenKind::InlineTableClose))
        .count()
}

#[cfg(test)]
mod test {
    use snapbox::assert_data_eq;
    use snapbox::str;
    use snapbox::IntoData;

    use crate::config::options::IndentStyle;

    #[track_caller]
    fn valid(
        input: &str,
        hard_tabs: bool,
        tab_spaces: usize,
        indent_style: IndentStyle,
        expected: impl IntoData,
    ) {
        let mut tokens = crate::toml::TomlTokens::parse(input);
        super::normalize_indent(&mut tokens, hard_tabs, tab_spaces, indent_style);
        let actual = tokens.to_string();

        assert_data_eq!(&actual, expected);

        let (_, errors) = toml::de::DeTable::parse_recoverable(&actual);
        if !errors.is_empty() {
            use std::fmt::Write as _;
            let mut result = String::new();
            writeln!(&mut result, "---").unwrap();
            for error in errors {
                writeln!(&mut result, "{error}").unwrap();
                writeln!(&mut result, "---").unwrap();
            }
            panic!("failed to parse\n---\n{actual}\n{result}");
        }
    }

    #[test]
    fn empty_tabs() {
        valid("", true, 10, IndentStyle::Block, str![]);
    }

    #[test]
    fn empty_spaces() {
        valid("", false, 10, IndentStyle::Block, str![]);
    }

    #[test]
    fn cleanup_tabs() {
        valid(
            "
  a = 5

  # Hello

  [b]
  a = 10
  b = [
    1,
    2,
    3,
  ]
  c = [
    [
      1,
      2,
      3,
    ]
  ]
  d = [[
      1,
      2,
      3,
  ]]

  [e]
    f = 10

g = 11
",
            true,
            10,
            IndentStyle::Block,
            str![[r#"

a = 5

# Hello

[b]
a = 10
b = [
	1,
	2,
	3,
]
c = [
	[
		1,
		2,
		3,
	]
]
d = [[
		1,
		2,
		3,
]]

[e]
f = 10

g = 11

"#]],
        );
    }

    #[test]
    fn cleanup_spaces() {
        valid(
            "
  a = 5

  # Hello

  [b]
  a = 10
  b = [
    1,
    2,
    3,
  ]
  c = [
    [
      1,
      2,
      3,
    ]
  ]
  d = [[
      1,
      2,
      3,
  ]]

  [e]
    f = 10

g = 11
",
            false,
            10,
            IndentStyle::Block,
            str![[r#"

a = 5

# Hello

[b]
a = 10
b = [
          1,
          2,
          3,
]
c = [
          [
                    1,
                    2,
                    3,
          ]
]
d = [[
                    1,
                    2,
                    3,
]]

[e]
f = 10

g = 11

"#]],
        );
    }

    #[test]
    fn block_from_visual_simple() {
        // Block style adjusts indentation but preserves structure
        // (reflow_arrays handles structure decisions based on array_width)
        valid(
            r#"
b = [1,
     2,
     3]
"#,
            false,
            4,
            IndentStyle::Block,
            str![[r#"

b = [1,
    2,
    3]

"#]],
        );
    }

    #[test]
    fn block_from_visual_nested() {
        // Block style adjusts indentation but preserves structure
        valid(
            r#"
c = [[1,
      2],
     [3,
      4]]
"#,
            false,
            4,
            IndentStyle::Block,
            str![[r#"

c = [[1,
        2],
    [3,
        4]]

"#]],
        );
    }

    #[test]
    fn block_from_visual_with_trailing_comma() {
        // Block style adjusts indentation but preserves structure
        valid(
            r#"
deps = ["foo",
        "bar",]
"#,
            false,
            4,
            IndentStyle::Block,
            str![[r#"

deps = ["foo",
    "bar",]

"#]],
        );
    }

    #[test]
    fn visual_simple_array() {
        // Visual style: first element on same line, subsequent elements align
        // Trailing comma is removed (matches rustfmt)
        valid(
            r#"
b = [
    1,
    2,
]
"#,
            false,
            4,
            IndentStyle::Visual,
            str![[r#"

b = [1,
     2]

"#]],
        );
    }

    #[test]
    fn visual_nested_arrays() {
        // Visual style: first element on same line, nested arrays align
        // Trailing commas are removed (matches rustfmt)
        valid(
            r#"
c = [
    [
        1,
        2,
    ]
]
"#,
            false,
            4,
            IndentStyle::Visual,
            str![[r#"

c = [[1,
      2]]

"#]],
        );
    }

    #[test]
    fn visual_longer_key() {
        // Visual style: first element on same line, aligns at column 16
        // Trailing comma is removed (matches rustfmt)
        valid(
            r#"
dependencies = [
    "foo",
    "bar",
]
"#,
            false,
            4,
            IndentStyle::Visual,
            str![[r#"

dependencies = ["foo",
                "bar"]

"#]],
        );
    }

    #[test]
    fn visual_ignores_hard_tabs_setting() {
        // Visual style always uses spaces for alignment regardless of hard_tabs setting
        // Trailing comma is removed (matches rustfmt)
        valid(
            r#"
b = [
    1,
    2,
]
"#,
            true,
            4,
            IndentStyle::Visual,
            str![[r#"

b = [1,
     2]

"#]],
        );
    }

    #[test]
    fn visual_deeply_nested() {
        // Visual style: first element on same line at each nesting level
        // Trailing commas are removed (matches rustfmt)
        valid(
            r#"
matrix = [
    [
        [1, 2],
        [3, 4],
    ],
]
"#,
            false,
            4,
            IndentStyle::Visual,
            str![[r#"

matrix = [[[1, 2],
           [3, 4]]]

"#]],
        );
    }

    #[test]
    fn visual_with_comments() {
        // Visual style: comment becomes first element on same line
        // Trailing comma is removed (matches rustfmt)
        valid(
            r#"
deps = [
    # first item
    "foo",
    # second item
    "bar",
]
"#,
            false,
            4,
            IndentStyle::Visual,
            str![[r#"

deps = [# first item
        "foo",
        # second item
        "bar"]

"#]],
        );
    }

    #[test]
    fn visual_empty_array() {
        // Visual style: empty arrays collapse to single line
        valid(
            r#"
a = []
b = [
]
"#,
            false,
            4,
            IndentStyle::Visual,
            str![[r#"

a = []
b = []

"#]],
        );
    }

    #[test]
    fn visual_no_trailing_comma() {
        // Visual style: without trailing comma, closer on same line as last element
        // Matches rustfmt Visual style: vec!["ipsum", "dolor", "sit"];
        valid(
            r#"
deps = [
    "ipsum",
    "dolor",
    "sit"
]
"#,
            false,
            4,
            IndentStyle::Visual,
            str![[r#"

deps = ["ipsum",
        "dolor",
        "sit"]

"#]],
        );
    }

    #[test]
    fn visual_preserves_table_structure() {
        // Visual style: first element on same line, preserves table structure
        // Trailing comma is removed (matches rustfmt)
        valid(
            r#"
[package]
name = "test"
deps = [
    "foo",
]

[dependencies]
bar = "1.0"
"#,
            false,
            4,
            IndentStyle::Visual,
            str![[r#"

[package]
name = "test"
deps = ["foo"]

[dependencies]
bar = "1.0"

"#]],
        );
    }
}
