//! A `/** */` comment's tokens: its lines past each one's `*`, as `@microsoft/tsdoc`'s
//! LineExtractor finds them, split the way its Tokenizer splits them.

/// What a token is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    /// Past the last line. Only the last token is one.
    EndOfInput,
    /// The end of a line.
    Newline,
    /// Spaces and tabs.
    Spacing,
    /// ASCII letters, digits and `_`.
    AsciiWord,
    /// A punctuation character no other kind names.
    OtherPunctuation,
    /// Anything else, such as other letters.
    Other,
    Backslash,
    LessThan,
    GreaterThan,
    Equals,
    SingleQuote,
    DoubleQuote,
    Slash,
    Hyphen,
    AtSign,
    LeftCurly,
    RightCurly,
    Backtick,
    Period,
    Colon,
    Comma,
    LeftSquare,
    RightSquare,
    Pipe,
    LeftParen,
    RightParen,
    Pound,
    Plus,
    Dollar,
}

impl Kind {
    /// The kind of `c`'s token.
    fn of(c: char) -> Self {
        match c {
            ' ' | '\t' => Self::Spacing,
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' => Self::AsciiWord,
            '\\' => Self::Backslash,
            '<' => Self::LessThan,
            '>' => Self::GreaterThan,
            '=' => Self::Equals,
            '\'' => Self::SingleQuote,
            '"' => Self::DoubleQuote,
            '/' => Self::Slash,
            '-' => Self::Hyphen,
            '@' => Self::AtSign,
            '{' => Self::LeftCurly,
            '}' => Self::RightCurly,
            '`' => Self::Backtick,
            '.' => Self::Period,
            ':' => Self::Colon,
            ',' => Self::Comma,
            '[' => Self::LeftSquare,
            ']' => Self::RightSquare,
            '|' => Self::Pipe,
            '(' => Self::LeftParen,
            ')' => Self::RightParen,
            '#' => Self::Pound,
            '+' => Self::Plus,
            '$' => Self::Dollar,
            c if c.is_ascii_punctuation() => Self::OtherPunctuation,
            _ => Self::Other,
        }
    }

    /// Whether a run of these characters is one token.
    fn runs(self) -> bool {
        matches!(self, Self::Spacing | Self::AsciiWord | Self::Other)
    }

    /// Whether it's CommonMark punctuation, which a backslash escapes.
    pub(super) fn punctuation(self) -> bool {
        !matches!(self, Self::EndOfInput | Self::Newline | Self::Spacing | Self::AsciiWord | Self::Other)
    }
}

/// A token, at byte offsets into the comment.
#[derive(Clone, Copy, Debug)]
pub(super) struct Token {
    pub kind: Kind,
    pub start: u32,
    pub end: u32,
}

/// The tokens of `comment`, a whole `/** */` comment: each line's, then a newline, and an
/// end-of-input token last.
pub(super) fn tokens(comment: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut last = 0;
    for (start, end) in lines(comment) {
        let mut run: Option<(Kind, usize)> = None;
        for (at, c) in comment[start..end].char_indices() {
            let (kind, at) = (Kind::of(c), start + at);
            match run {
                Some((same, _)) if same == kind && kind.runs() => {}
                _ => {
                    if let Some((kind, from)) = run {
                        tokens.push(Token { kind, start: from as u32, end: at as u32 });
                    }
                    run = Some((kind, at));
                }
            }
        }
        if let Some((kind, from)) = run {
            tokens.push(Token { kind, start: from as u32, end: end as u32 });
        }
        tokens.push(Token { kind: Kind::Newline, start: end as u32, end: end as u32 });
        last = end;
    }
    tokens.push(Token { kind: Kind::EndOfInput, start: last as u32, end: last as u32 });
    tokens
}

/// Where each line of `comment`'s text starts and ends: past the `/**` or a line's leading `*`
/// and the one space after it, up to the line's last character that isn't a space. A first line
/// with nothing on it isn't one, and a line without a `*` keeps its leading spaces.
fn lines(comment: &str) -> Vec<(usize, usize)> {
    let bytes = comment.as_bytes();
    let space_at = |at: usize| usize::from(bytes.get(at) == Some(&b' '));
    let mut lines = Vec::new();
    // Past `/**` and a space after it.
    let mut start = 3 + space_at(3);
    let (mut end, mut first, mut advancing) = (start, true, false);
    let skip = start;
    let mut chars = comment.char_indices().skip_while(|&(at, _)| at < skip).peekable();
    while let Some((at, c)) = chars.next() {
        let closes = c == '*' && chars.peek().is_some_and(|&(_, next)| next == '/');
        if advancing {
            if c == '*' && !closes {
                // A line's leading `*`, and a space after it.
                start = at + 1 + space_at(at + 1);
                end = start;
                advancing = false;
                if start > at + 1 {
                    chars.next();
                }
            } else if c == '\n' {
                lines.push((at, at));
                start = at + 1;
            } else if !closes && !whitespace(c) {
                end = at + c.len_utf8();
                advancing = false;
            }
        } else if c == '\n' {
            if !first || end > start {
                lines.push((start, end));
            }
            (start, end, advancing, first) = (at + 1, at + 1, true, false);
        } else if !closes && !whitespace(c) {
            end = at + c.len_utf8();
        }
        if closes {
            if !advancing && end > start {
                lines.push((start, end));
            }
            break;
        }
    }
    lines
}

/// Whether `c` is whitespace to JavaScript's `\s`.
fn whitespace(c: char) -> bool {
    (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}'
}
