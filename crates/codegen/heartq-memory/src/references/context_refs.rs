//! Parse `@`-reference tokens from free-form text.

use std::path::PathBuf;

/// A parsed `@`-reference with its span in the source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedReference {
    pub reference: ContextReference,
    pub start: usize,
    pub end: usize,
}

/// Supported `@`-reference kinds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextReference {
    File { path: PathBuf },
    Folder { path: PathBuf },
    Diff,
    Staged,
    Git { n: u32 },
    Url { url: String },
}

/// Scan `text` for all `@`-references in document order.
pub fn parse_context_references(text: &str) -> Vec<ParsedReference> {
    let mut refs = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'@' {
            i += 1;
            continue;
        }
        if i > 0 {
            let prev = bytes[i - 1];
            if prev.is_ascii_alphanumeric() || prev == b'_' {
                i += 1;
                continue;
            }
        }
        let start = i;
        i += 1;
        if let Some(parsed) = parse_one(text, start, &mut i) {
            refs.push(parsed);
        }
    }
    refs
}

fn parse_one(text: &str, start: usize, cursor: &mut usize) -> Option<ParsedReference> {
    let rest = &text[*cursor..];
    if let Some(path) = strip_prefix(rest, "file:") {
        let (raw, consumed) = take_pathish(path);
        *cursor += "file:".len() + consumed;
        return Some(ParsedReference {
            reference: ContextReference::File {
                path: PathBuf::from(raw),
            },
            start,
            end: *cursor,
        });
    }
    if let Some(path) = strip_prefix(rest, "folder:") {
        let (raw, consumed) = take_pathish(path);
        *cursor += "folder:".len() + consumed;
        return Some(ParsedReference {
            reference: ContextReference::Folder {
                path: PathBuf::from(raw),
            },
            start,
            end: *cursor,
        });
    }
    if rest.starts_with("diff") && boundary_after(&rest[4..]) {
        *cursor += "diff".len();
        return Some(ParsedReference {
            reference: ContextReference::Diff,
            start,
            end: *cursor,
        });
    }
    if rest.starts_with("staged") && boundary_after(&rest[6..]) {
        *cursor += "staged".len();
        return Some(ParsedReference {
            reference: ContextReference::Staged,
            start,
            end: *cursor,
        });
    }
    if let Some(tail) = strip_prefix(rest, "git:") {
        let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            return None;
        }
        let n: u32 = digits.parse().ok()?;
        *cursor += "git:".len() + digits.len();
        return Some(ParsedReference {
            reference: ContextReference::Git { n },
            start,
            end: *cursor,
        });
    }
    if let Some(url_tail) = strip_prefix(rest, "url:") {
        let (url, consumed) = take_url(url_tail);
        if url.is_empty() {
            return None;
        }
        *cursor += "url:".len() + consumed;
        return Some(ParsedReference {
            reference: ContextReference::Url { url: url.to_string() },
            start,
            end: *cursor,
        });
    }
    None
}

fn strip_prefix<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    s.strip_prefix(prefix)
}

fn boundary_after(s: &str) -> bool {
    s.is_empty()
        || s.starts_with(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
}

fn take_pathish(s: &str) -> (&str, usize) {
    if s.starts_with('"') {
        if let Some(end) = s[1..].find('"') {
            let raw = &s[1..1 + end];
            return (raw, end + 2);
        }
    }
    let len = s
        .char_indices()
        .find(|(_, c)| c.is_whitespace() || matches!(*c, ',' | ')' | ']' | '}' | ';'))
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    (&s[..len], len)
}

fn take_url(s: &str) -> (&str, usize) {
    if s.starts_with('"') {
        if let Some(end) = s[1..].find('"') {
            return (&s[1..1 + end], end + 2);
        }
    }
    let len = s
        .char_indices()
        .find(|(_, c)| c.is_whitespace() || matches!(*c, ',' | ')' | ']' | '}' | ';'))
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    (&s[..len], len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_reference_kinds() {
        let text = "see @file:src/main.rs and @folder:docs/ plus @diff @staged @git:5 @url:https://example.com/x";
        let refs = parse_context_references(text);
        assert_eq!(refs.len(), 6);
        assert!(matches!(refs[0].reference, ContextReference::File { .. }));
        assert!(matches!(refs[1].reference, ContextReference::Folder { .. }));
        assert!(matches!(refs[2].reference, ContextReference::Diff));
        assert!(matches!(refs[3].reference, ContextReference::Staged));
        assert!(matches!(refs[4].reference, ContextReference::Git { n: 5 }));
        assert!(matches!(refs[5].reference, ContextReference::Url { .. }));
    }

    #[test]
    fn ignores_email_addresses() {
        let refs = parse_context_references("mail user@example.com today");
        assert!(refs.is_empty());
    }
}
