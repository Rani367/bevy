//! A small, dependency-free Rust lexer for the in-editor code view's syntax highlighting.
//!
//! [`tokenize`] returns a list of byte ranges that **contiguously cover the whole source** (every
//! byte belongs to exactly one span), each tagged with a [`SyntaxKind`]. The code editor renders
//! one colored `TextSpan` per range behind a glyph-transparent `EditableText`; because the spans
//! reproduce the source verbatim (whitespace and newlines included), the colored layer lays out
//! identically to the editor and stays aligned.

use core::ops::Range;

/// The lexical category of a source span, mapped to a theme color by the code editor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyntaxKind {
    /// Identifiers, whitespace, and anything uncategorized.
    Normal,
    /// A Rust keyword (`fn`, `let`, `impl`, …).
    Keyword,
    /// A type-like identifier (starts uppercase).
    Type,
    /// An identifier used as a function call (`foo(`).
    Function,
    /// A macro invocation (`println!`).
    Macro,
    /// A string literal (including raw strings).
    Str,
    /// A character literal.
    Char,
    /// A numeric literal.
    Number,
    /// A line or block comment.
    Comment,
    /// A lifetime (`'a`).
    Lifetime,
    /// An attribute (`#[...]` / `#![...]`).
    Attribute,
    /// Punctuation / operators.
    Punct,
}

const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type", "union",
    "unsafe", "use", "where", "while",
];

fn byte_at(chars: &[(usize, char)], total: usize, k: usize) -> usize {
    if k < chars.len() {
        chars[k].0
    } else {
        total
    }
}

/// Tokenize `src` into contiguous, kind-tagged byte ranges covering the entire string.
pub fn tokenize(src: &str) -> Vec<(Range<usize>, SyntaxKind)> {
    let chars: Vec<(usize, char)> = src.char_indices().collect();
    let n = chars.len();
    let total = src.len();
    let mut out: Vec<(Range<usize>, SyntaxKind)> = Vec::new();
    let mut k = 0usize;

    macro_rules! emit {
        ($a:expr, $b:expr, $kind:expr) => {
            if $a < $b {
                out.push((
                    byte_at(&chars, total, $a)..byte_at(&chars, total, $b),
                    $kind,
                ));
            }
        };
    }

    while k < n {
        let c = chars[k].1;

        // Whitespace.
        if c.is_whitespace() {
            let a = k;
            while k < n && chars[k].1.is_whitespace() {
                k += 1;
            }
            emit!(a, k, SyntaxKind::Normal);
            continue;
        }

        // Line comment.
        if c == '/' && chars.get(k + 1).map(|x| x.1) == Some('/') {
            let a = k;
            while k < n && chars[k].1 != '\n' {
                k += 1;
            }
            emit!(a, k, SyntaxKind::Comment);
            continue;
        }

        // Block comment (nested).
        if c == '/' && chars.get(k + 1).map(|x| x.1) == Some('*') {
            let a = k;
            k += 2;
            let mut depth = 1;
            while k < n && depth > 0 {
                if chars[k].1 == '/' && chars.get(k + 1).map(|x| x.1) == Some('*') {
                    depth += 1;
                    k += 2;
                } else if chars[k].1 == '*' && chars.get(k + 1).map(|x| x.1) == Some('/') {
                    depth -= 1;
                    k += 2;
                } else {
                    k += 1;
                }
            }
            emit!(a, k, SyntaxKind::Comment);
            continue;
        }

        // Raw string: r"..." / r#"..."# (with any number of hashes).
        if c == 'r' && matches!(chars.get(k + 1).map(|x| x.1), Some('"') | Some('#')) {
            let a = k;
            let mut j = k + 1;
            let mut hashes = 0;
            while j < n && chars[j].1 == '#' {
                hashes += 1;
                j += 1;
            }
            if j < n && chars[j].1 == '"' {
                j += 1;
                while j < n {
                    if chars[j].1 == '"' {
                        let mut h = 0;
                        let mut m = j + 1;
                        while m < n && h < hashes && chars[m].1 == '#' {
                            h += 1;
                            m += 1;
                        }
                        if h == hashes {
                            j = m;
                            break;
                        }
                    }
                    j += 1;
                }
                k = j;
                emit!(a, k, SyntaxKind::Str);
                continue;
            }
            // Not actually a raw string — fall through to identifier handling.
        }

        // String literal.
        if c == '"' {
            let a = k;
            k += 1;
            while k < n {
                match chars[k].1 {
                    '\\' => k += 2,
                    '"' => {
                        k += 1;
                        break;
                    }
                    _ => k += 1,
                }
            }
            emit!(a, k, SyntaxKind::Str);
            continue;
        }

        // Char literal or lifetime.
        if c == '\'' {
            let a = k;
            if chars.get(k + 1).map(|x| x.1) == Some('\\') {
                // Escaped char literal: '\n', '\'', '\u{...}', …
                let mut j = k + 1;
                while j < n && chars[j].1 != '\'' && chars[j].1 != '\n' {
                    j += 1;
                }
                if j < n && chars[j].1 == '\'' {
                    k = j + 1;
                    emit!(a, k, SyntaxKind::Char);
                    continue;
                }
            } else if chars.get(k + 2).map(|x| x.1) == Some('\'') {
                // Simple char literal: 'x'
                k += 3;
                emit!(a, k, SyntaxKind::Char);
                continue;
            }
            // Otherwise a lifetime: 'ident
            k += 1;
            while k < n && (chars[k].1.is_alphanumeric() || chars[k].1 == '_') {
                k += 1;
            }
            emit!(a, k, SyntaxKind::Lifetime);
            continue;
        }

        // Attribute: #[...] / #![...]
        if c == '#' && matches!(chars.get(k + 1).map(|x| x.1), Some('[') | Some('!')) {
            let a = k;
            let mut j = k + 1;
            if chars[j].1 == '!' {
                j += 1;
            }
            if j < n && chars[j].1 == '[' {
                let mut depth = 0;
                while j < n {
                    match chars[j].1 {
                        '[' => depth += 1,
                        ']' => {
                            depth -= 1;
                            if depth == 0 {
                                j += 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
                k = j;
                emit!(a, k, SyntaxKind::Attribute);
                continue;
            }
            // Otherwise fall through to punctuation.
        }

        // Number.
        if c.is_ascii_digit() {
            let a = k;
            k += 1;
            while k < n {
                let ch = chars[k].1;
                if ch == '.' {
                    // Include a decimal point, but not a `..` range operator.
                    if chars.get(k + 1).map(|x| x.1.is_ascii_digit()) == Some(true) {
                        k += 1;
                    } else {
                        break;
                    }
                } else if ch.is_alphanumeric() || ch == '_' {
                    k += 1;
                } else {
                    break;
                }
            }
            emit!(a, k, SyntaxKind::Number);
            continue;
        }

        // Identifier / keyword / type / function / macro.
        if c.is_alphabetic() || c == '_' {
            let a = k;
            k += 1;
            while k < n && (chars[k].1.is_alphanumeric() || chars[k].1 == '_') {
                k += 1;
            }
            let word: String = chars[a..k].iter().map(|(_, ch)| *ch).collect();
            let next = chars.get(k).map(|x| x.1);
            let kind = if KEYWORDS.contains(&word.as_str()) {
                SyntaxKind::Keyword
            } else if next == Some('!') {
                SyntaxKind::Macro
            } else if word.chars().next().is_some_and(char::is_uppercase) {
                SyntaxKind::Type
            } else if next == Some('(') {
                SyntaxKind::Function
            } else {
                SyntaxKind::Normal
            };
            emit!(a, k, kind);
            continue;
        }

        // Anything else: punctuation / operator.
        let a = k;
        k += 1;
        emit!(a, k, SyntaxKind::Punct);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::{tokenize, SyntaxKind};

    /// The token ranges must reconstruct the source exactly (contiguous, gapless coverage).
    fn assert_covers(src: &str) {
        let toks = tokenize(src);
        let mut rebuilt = String::new();
        let mut prev_end = 0;
        for (r, _) in &toks {
            assert_eq!(r.start, prev_end, "spans must be contiguous");
            rebuilt.push_str(&src[r.clone()]);
            prev_end = r.end;
        }
        assert_eq!(prev_end, src.len(), "spans must cover the whole source");
        assert_eq!(rebuilt, src);
    }

    fn kind_of<'a>(
        src: &'a str,
        toks: &[(core::ops::Range<usize>, SyntaxKind)],
        needle: &str,
    ) -> SyntaxKind {
        let at = src.find(needle).expect("substring present");
        toks.iter()
            .find(|(r, _)| r.start == at)
            .map(|(_, k)| *k)
            .unwrap_or_else(|| panic!("no token starting at {needle:?}"))
    }

    #[test]
    fn covers_simple_source() {
        assert_covers("fn main() {\n    let x = 5; // hi\n}\n");
        assert_covers("let s = \"a\\\"b\";\n");
        assert_covers("");
        assert_covers("   \n\t  ");
        assert_covers("#[derive(Debug)]\nstruct S<'a> { x: &'a str }\n");
    }

    #[test]
    fn classifies_common_tokens() {
        let src = "fn main() { let x = 42; println!(\"hi\"); }";
        let toks = tokenize(src);
        assert_eq!(kind_of(src, &toks, "fn"), SyntaxKind::Keyword);
        assert_eq!(kind_of(src, &toks, "let"), SyntaxKind::Keyword);
        assert_eq!(kind_of(src, &toks, "main"), SyntaxKind::Function);
        assert_eq!(kind_of(src, &toks, "42"), SyntaxKind::Number);
        assert_eq!(kind_of(src, &toks, "println"), SyntaxKind::Macro);
        assert_eq!(kind_of(src, &toks, "\"hi\""), SyntaxKind::Str);
    }

    #[test]
    fn classifies_types_comments_lifetimes() {
        let src = "// note\nstruct Foo<'a>(&'a u8); /* block */";
        let toks = tokenize(src);
        assert_eq!(kind_of(src, &toks, "// note"), SyntaxKind::Comment);
        assert_eq!(kind_of(src, &toks, "struct"), SyntaxKind::Keyword);
        assert_eq!(kind_of(src, &toks, "Foo"), SyntaxKind::Type);
        assert_eq!(kind_of(src, &toks, "'a>"), SyntaxKind::Lifetime);
        assert_eq!(kind_of(src, &toks, "/* block */"), SyntaxKind::Comment);
    }

    #[test]
    fn raw_and_char_literals() {
        let src = "let r = r#\"a\"b\"#; let c = 'x';";
        let toks = tokenize(src);
        assert_eq!(kind_of(src, &toks, "r#\"a\"b\"#"), SyntaxKind::Str);
        assert_eq!(kind_of(src, &toks, "'x'"), SyntaxKind::Char);
        assert_covers(src);
    }
}
