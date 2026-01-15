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
    // IndentStyle::Visual not yet implemented - falls back to Block behavior
    let _ = indent_style;
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
                let next_i = i + 1;
                if let Some(next) = tokens.tokens.get_mut(next_i) {
                    match (next.kind, depth) {
                        (TokenKind::Newline, _) => {}
                        (TokenKind::Whitespace, 0) => {
                            *next = TomlToken::EMPTY;
                        }
                        (TokenKind::Whitespace, _) => {
                            let close_count = close_count(tokens, next_i);
                            let ws = buffer.whitespace(depth - close_count);
                            let mut token = TomlToken::EMPTY;
                            token.raw = Cow::Owned(ws.to_owned());
                            tokens.tokens[next_i] = token;
                        }
                        (_, 0) => {}
                        (_, _) => {
                            let close_count = close_count(tokens, next_i);
                            let ws = buffer.whitespace(depth - close_count);
                            let mut token = TomlToken::EMPTY;
                            token.raw = Cow::Owned(ws.to_owned());
                            tokens.tokens.insert(next_i, token);
                        }
                    }
                }
            }
            TokenKind::Error => {}
        }
    }
    tokens.trim_empty_whitespace();
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
        // Visual style currently delegates to Block behavior
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

b = [
    1,
    2,
]

"#]],
        );
    }

    #[test]
    fn visual_nested_arrays() {
        // Visual style currently delegates to Block behavior
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

c = [
    [
        1,
        2,
    ]
]

"#]],
        );
    }

    #[test]
    fn visual_longer_key() {
        // Visual style currently delegates to Block behavior
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

dependencies = [
    "foo",
    "bar",
]

"#]],
        );
    }

    #[test]
    fn visual_ignores_hard_tabs_setting() {
        // Visual style currently delegates to Block behavior
        // With hard_tabs=true, currently produces tabs (will use spaces when implemented)
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

b = [
	1,
	2,
]

"#]],
        );
    }

    #[test]
    fn visual_deeply_nested() {
        // Visual style currently delegates to Block behavior
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

matrix = [
    [
        [1, 2],
        [3, 4],
    ],
]

"#]],
        );
    }

    #[test]
    fn visual_with_comments() {
        // Visual style currently delegates to Block behavior
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

deps = [
    # first item
    "foo",
    # second item
    "bar",
]

"#]],
        );
    }

    #[test]
    fn visual_empty_array() {
        // Visual style currently delegates to Block behavior
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
b = [
]

"#]],
        );
    }

    #[test]
    fn visual_no_trailing_comma() {
        // Visual style currently delegates to Block behavior
        // When implemented, closer should be on same line as last element (rustfmt style)
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

deps = [
    "ipsum",
    "dolor",
    "sit"
]

"#]],
        );
    }

    #[test]
    fn visual_preserves_table_structure() {
        // Visual style currently delegates to Block behavior
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
deps = [
    "foo",
]

[dependencies]
bar = "1.0"

"#]],
        );
    }
}
