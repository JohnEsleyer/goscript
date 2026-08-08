#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Identifier(String),
    Type(String),
    Int(i64),
    Float(f64),
    Str(String),
    Var,
    Func,
    If,
    Else,
    For,
    Return,
    Break,
    Continue,
    True,
    False,
    Nil,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Assign,
    PlusAssign,
    MinusAssign,
    StarAssign,
    SlashAssign,
    ShortAssign,
    PlusPlus,
    MinusMinus,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    And,
    Or,
    Not,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Semicolon,
    Dot,
    Colon,
    Struct,
    KwType,
    Switch,
    Case,
    Default,
    Eof,
}

impl TokenKind {
    pub fn describe(&self) -> String {
        match self {
            TokenKind::Identifier(_) => "identifier".into(),
            TokenKind::Type(t) => format!("type '{}'", t),
            TokenKind::Int(_) => "integer literal".into(),
            TokenKind::Float(_) => "float literal".into(),
            TokenKind::Str(_) => "string literal".into(),
            TokenKind::Var => "'var'".into(),
            TokenKind::Func => "'func'".into(),
            TokenKind::If => "'if'".into(),
            TokenKind::Else => "'else'".into(),
            TokenKind::For => "'for'".into(),
            TokenKind::Return => "'return'".into(),
            TokenKind::Break => "'break'".into(),
            TokenKind::Continue => "'continue'".into(),
            TokenKind::True => "'true'".into(),
            TokenKind::False => "'false'".into(),
            TokenKind::Nil => "'nil'".into(),
            TokenKind::Plus => "'+'".into(),
            TokenKind::Minus => "'-'".into(),
            TokenKind::Star => "'*'".into(),
            TokenKind::Slash => "'/'".into(),
            TokenKind::Percent => "'%'".into(),
            TokenKind::Assign => "'='".into(),
            TokenKind::PlusAssign => "'+='".into(),
            TokenKind::MinusAssign => "'-='".into(),
            TokenKind::StarAssign => "'*='".into(),
            TokenKind::SlashAssign => "'/='".into(),
            TokenKind::ShortAssign => "':='".into(),
            TokenKind::PlusPlus => "'++'".into(),
            TokenKind::MinusMinus => "'--'".into(),
            TokenKind::Equal => "'=='".into(),
            TokenKind::NotEqual => "'!='".into(),
            TokenKind::Less => "'<'".into(),
            TokenKind::LessEqual => "'<='".into(),
            TokenKind::Greater => "'>'".into(),
            TokenKind::GreaterEqual => "'>='".into(),
            TokenKind::And => "'&&'".into(),
            TokenKind::Or => "'||'".into(),
            TokenKind::Not => "'!'".into(),
            TokenKind::LParen => "'('".into(),
            TokenKind::RParen => "')'".into(),
            TokenKind::LBrace => "'{'".into(),
            TokenKind::RBrace => "'}'".into(),
            TokenKind::LBracket => "'['".into(),
            TokenKind::RBracket => "']'".into(),
            TokenKind::Comma => "','".into(),
            TokenKind::Semicolon => "';'".into(),
            TokenKind::Dot => "'.'".into(),
            TokenKind::Colon => "':'".into(),
            TokenKind::Struct => "'struct'".into(),
            TokenKind::KwType => "'type'".into(),
            TokenKind::Switch => "'switch'".into(),
            TokenKind::Case => "'case'".into(),
            TokenKind::Default => "'default'".into(),
            TokenKind::Eof => "end of file".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub col: usize,
}

const TYPE_WORDS: &[&str] = &[
    "bool", "byte", "complex64", "complex128", "error", "float32", "float64", "int",
    "int8", "int16", "int32", "int64", "rune", "string", "uint", "uint8", "uint16",
    "uint32", "uint64", "uintptr",
];

const CONTINUATION_CHARS: &[char] = &[
    '+', '-', '*', '/', '%', '<', '>', '=', '&', '|', '!', ':', '(', ',', ')', '.', '[',
];

pub struct Lexer<'a> {
    input: &'a str,
    line: usize,
    col: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self { input, line: 1, col: 1 }
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, crate::error::Error> {
        let mut tokens = Vec::new();
        let mut can_end_stmt = false;
        let mut chars = self.input.chars().peekable();

        while let Some(ch) = chars.next() {
            let (line, col) = (self.line, self.col);
            match ch {
                '\n' => {
                    if can_end_stmt && !next_word_continues(&mut chars) {
                        tokens.push(Token { kind: TokenKind::Semicolon, line, col });
                    }
                    can_end_stmt = false;
                    self.line += 1;
                    self.col = 1;
                }
                ' ' | '\t' | '\r' => {
                    self.col += 1;
                }
                '/' => {
                    if chars.peek() == Some(&'/') {
                        self.col += 2;
                        for cb in chars.by_ref() {
                            if cb == '\n' {
                                break;
                            }
                            self.col += 1;
                        }
                    } else if chars.peek() == Some(&'*') {
                        self.col += 2;
                        chars.next();
                        self.skip_block_comment(&mut chars)?;
                    } else if chars.peek() == Some(&'=') {
                        self.col += 2;
                        chars.next();
                        can_end_stmt = false;
                        tokens.push(Token {
                            kind: TokenKind::SlashAssign,
                            line,
                            col,
                        });
                    } else {
                        self.col += 1;
                        can_end_stmt = false;
                        tokens.push(Token { kind: TokenKind::Slash, line, col });
                    }
                }
                '"' => {
                    self.col += 1;
                    let s = self.lex_string(&mut chars)?;
                    can_end_stmt = true;
                    tokens.push(Token { kind: TokenKind::Str(s), line, col });
                }
                c if c.is_ascii_digit() => {
                    self.col += 1;
                    let (int_part, float_part) = self.lex_number(&mut chars)?;
                    can_end_stmt = true;
                    if !float_part.is_empty() {
                        let mut num = int_part;
                        num.push_str(&float_part);
                        num.insert(0, c);
                        let v = num.parse::<f64>().map_err(|_| {
                            crate::error::Error::new("invalid floating-point literal", line, col)
                        })?;
                        tokens.push(Token { kind: TokenKind::Float(v), line, col });
                    } else {
                        let mut num = int_part;
                        num.insert(0, c);
                        let v = num.parse::<i64>().map_err(|_| {
                            crate::error::Error::new("integer literal out of range", line, col)
                        })?;
                        tokens.push(Token { kind: TokenKind::Int(v), line, col });
                    }
                }
                c if is_ident_start(c) => {
                    let ident = self.lex_identifier(&mut chars, c);
                    let kind = keyword_or_ident(ident);
                    can_end_stmt = matches!(
                        &kind,
                        TokenKind::Identifier(_)
                            | TokenKind::True
                            | TokenKind::False
                            | TokenKind::Return
                            | TokenKind::Break
                            | TokenKind::Continue
                    );
                    tokens.push(Token { kind, line, col });
                }
                _ => {
                    self.col += 1;
                    let kind = self.lex_operator(ch, &mut chars, line, col)?;
                    can_end_stmt = matches!(
                        kind,
                        TokenKind::RParen | TokenKind::RBrace | TokenKind::RBracket
                    );
                    tokens.push(Token { kind, line, col });
                }
            }
        }

        if can_end_stmt {
            tokens.push(Token {
                kind: TokenKind::Semicolon,
                line: self.line,
                col: self.col,
            });
        }
        tokens.push(Token { kind: TokenKind::Eof, line: self.line, col: self.col });
        Ok(tokens)
    }

    fn skip_block_comment(
        &mut self,
        chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    ) -> Result<(), crate::error::Error> {
        let mut prev = '\0';
        loop {
            match chars.next() {
                None => {
                    return Err(crate::error::Error::new(
                        "unterminated block comment",
                        self.line,
                        self.col,
                    ))
                }
                Some(c) if prev == '*' && c == '/' => {
                    self.col += 1;
                    return Ok(());
                }
                Some('\n') => {
                    self.line += 1;
                    self.col = 1;
                    prev = '\n';
                }
                Some(c) => {
                    self.col += 1;
                    prev = c;
                }
            }
        }
    }

    fn lex_string(
        &mut self,
        chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    ) -> Result<String, crate::error::Error> {
        let (line, col) = (self.line, self.col);
        let mut out = String::new();
        loop {
            match chars.next() {
                None => {
                    return Err(crate::error::Error::new(
                        "unterminated string literal",
                        line,
                        col,
                    ))
                }
                Some('"') => {
                    self.col += 1;
                    return Ok(out);
                }
                Some('\\') => {
                    match chars.next() {
                        Some('n') => out.push('\n'),
                        Some('t') => out.push('\t'),
                        Some('r') => out.push('\r'),
                        Some('0') => out.push('\0'),
                        Some('\\') => out.push('\\'),
                        Some('"') => out.push('"'),
                        Some(other) => {
                            out.push(other);
                        }
                        None => {
                            return Err(crate::error::Error::new(
                                "unterminated string literal",
                                line,
                                col,
                            ))
                        }
                    }
                    self.col += 2;
                }
                Some(c) => {
                    out.push(c);
                    self.col += 1;
                    if c == '\n' {
                        self.line += 1;
                        self.col = 1;
                    }
                }
            }
        }
    }

    fn lex_number(
        &mut self,
        chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    ) -> Result<(String, String), crate::error::Error> {
        let mut int_part = String::new();
        let mut float_part = String::new();
        let mut saw_dot = false;

        while let Some(&c) = chars.peek() {
            if c.is_ascii_digit() {
                if saw_dot {
                    float_part.push(c);
                } else {
                    int_part.push(c);
                }
                chars.next();
                self.col += 1;
            } else if c == '.' && !saw_dot {
                saw_dot = true;
                float_part.push(c);
                chars.next();
                self.col += 1;
            } else {
                break;
            }
        }

        if saw_dot && float_part == "." && int_part.is_empty() {
            return Err(crate::error::Error::new(
                "malformed floating-point literal",
                self.line,
                self.col,
            ));
        }
        Ok((int_part, float_part))
    }

    fn lex_identifier(
        &mut self,
        chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
        first: char,
    ) -> String {
        let mut ident = String::new();
        ident.push(first);
        self.col += 1;
        while let Some(&nc) = chars.peek() {
            if !is_ident_continue(nc) {
                break;
            }
            ident.push(nc);
            chars.next();
            self.col += 1;
        }
        ident
    }

    fn lex_operator(
        &mut self,
        c: char,
        chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
        line: usize,
        col: usize,
    ) -> Result<TokenKind, crate::error::Error> {
        let next = chars.peek().copied();

        macro_rules! two_char {
            ($t:expr) => {{
                chars.next();
                self.col += 1;
                $t
            }};
        }

        let kind = match c {
            '+' if next == Some('=') => two_char!(TokenKind::PlusAssign),
            '-' if next == Some('=') => two_char!(TokenKind::MinusAssign),
            '*' if next == Some('=') => two_char!(TokenKind::StarAssign),
            '/' if next == Some('=') => two_char!(TokenKind::SlashAssign),
            '+' if next == Some('+') => two_char!(TokenKind::PlusPlus),
            '-' if next == Some('-') => two_char!(TokenKind::MinusMinus),
            ':' if next == Some('=') => two_char!(TokenKind::ShortAssign),
            '=' if next == Some('=') => two_char!(TokenKind::Equal),
            '!' if next == Some('=') => two_char!(TokenKind::NotEqual),
            '<' if next == Some('=') => two_char!(TokenKind::LessEqual),
            '>' if next == Some('=') => two_char!(TokenKind::GreaterEqual),
            '&' if next == Some('&') => two_char!(TokenKind::And),
            '|' if next == Some('|') => two_char!(TokenKind::Or),
            '+' => TokenKind::Plus,
            '-' => TokenKind::Minus,
            '*' => TokenKind::Star,
            '%' => TokenKind::Percent,
            '=' => TokenKind::Assign,
            '<' => TokenKind::Less,
            '>' => TokenKind::Greater,
            '!' => TokenKind::Not,
            '(' => TokenKind::LParen,
            ')' => TokenKind::RParen,
            '{' => TokenKind::LBrace,
            '}' => TokenKind::RBrace,
            '[' => TokenKind::LBracket,
            ']' => TokenKind::RBracket,
            ',' => TokenKind::Comma,
            ';' => TokenKind::Semicolon,
            '.' => TokenKind::Dot,
            ':' if next == Some(':') => TokenKind::Colon,
            ':' => TokenKind::Colon,
            other => {
                return Err(crate::error::Error::new(
                    format!("unexpected character '{}'", other),
                    line,
                    col,
                ))
            }
        };
        Ok(kind)
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_continue(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn keyword_or_ident(word: String) -> TokenKind {
    match word.as_str() {
        "var" => TokenKind::Var,
        "func" => TokenKind::Func,
        "if" => TokenKind::If,
        "else" => TokenKind::Else,
        "for" => TokenKind::For,
        "return" => TokenKind::Return,
        "break" => TokenKind::Break,
        "continue" => TokenKind::Continue,
        "true" => TokenKind::True,
        "false" => TokenKind::False,
        "nil" => TokenKind::Nil,
        "type" => TokenKind::KwType,
        "struct" => TokenKind::Struct,
        "switch" => TokenKind::Switch,
        "case" => TokenKind::Case,
        "default" => TokenKind::Default,
        w if TYPE_WORDS.contains(&w) => TokenKind::Type(w.to_string()),
        _ => TokenKind::Identifier(word),
    }
}

fn next_word_continues(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> bool {
    let mut probe = chars.clone();
    loop {
        let c = match probe.next() {
            None => return false,
            Some(c) => c,
        };
        if c == '\n' || c.is_whitespace() {
            continue;
        }
        if c == '/' {
            if probe.peek() == Some(&'/') {
                for cb in probe.by_ref() {
                    if cb == '\n' {
                        break;
                    }
                }
                continue;
            }
            if probe.peek() == Some(&'*') {
                probe.next();
                let mut prev = '\0';
                loop {
                    match probe.next() {
                        None => return false,
                        Some(cb) if prev == '*' && cb == '/' => break,
                        Some(cb) => prev = cb,
                    }
                }
                continue;
            }
            return false;
        }
        if is_ident_start(c) {
            let mut word = String::new();
            word.push(c);
            probe.next();
            while let Some(&wc) = probe.peek() {
                if !is_ident_continue(wc) {
                    break;
                }
                word.push(wc);
                probe.next();
            }
            return word == "else";
        }
        return CONTINUATION_CHARS.contains(&c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

#[test]
    fn tokenizes_basic_script() {
        let src = "var x int = 12\nfunc Update(dt float64) {\n}\n";
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        let kinds: Vec<TokenKind> = tokens.into_iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Var,
                TokenKind::Identifier("x".into()),
                TokenKind::Type("int".into()),
                TokenKind::Assign,
                TokenKind::Int(12),
                TokenKind::Semicolon,
                TokenKind::Func,
                TokenKind::Identifier("Update".into()),
                TokenKind::LParen,
                TokenKind::Identifier("dt".into()),
                TokenKind::Type("float64".into()),
                TokenKind::RParen,
                TokenKind::LBrace,
                TokenKind::RBrace,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn int_literal_100() {
        let mut lexer = Lexer::new("var hp int = 100");
        let tokens = lexer.tokenize().unwrap();
        assert!(tokens.iter().any(|t| t.kind == TokenKind::Int(100)));
    }

    #[test]
    fn string_literal_with_spaces() {
        let mut lexer = Lexer::new("Log(\"hello world\")");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens.len(), 6);
        assert_eq!(tokens[1].kind, TokenKind::LParen);
        assert_eq!(tokens[2].kind, TokenKind::Str("hello world".to_string()));
    }

    #[test]
    fn line_comment_is_skipped() {
        let mut lexer = Lexer::new("var a = 1 // trailing\nvar b = 2");
        let tokens = lexer.tokenize().unwrap();
        let kinds: Vec<TokenKind> = tokens.into_iter().map(|t| t.kind).collect();
        assert_eq!(kinds[1], TokenKind::Identifier("a".to_string()));
        assert!(!kinds.contains(&TokenKind::Slash));
    }

    #[test]
    fn operators() {
        let mut lexer = Lexer::new("a := 1\nb += 2\nc == 3\nd && e\n");
        let tokens = lexer.tokenize().unwrap();
        let kinds: Vec<TokenKind> = tokens.into_iter().map(|t| t.kind).collect();
        assert!(kinds.contains(&TokenKind::ShortAssign));
        assert!(kinds.contains(&TokenKind::PlusAssign));
        assert!(kinds.contains(&TokenKind::Equal));
        assert!(kinds.contains(&TokenKind::And));
    }

    #[test]
    fn float_literals() {
        let mut lexer = Lexer::new("return 12.5 + 3.0");
        let tokens = lexer.tokenize().unwrap();
        let kinds: Vec<TokenKind> = tokens.into_iter().map(|t| t.kind).collect();
        assert!(kinds.contains(&TokenKind::Float(12.5)));
        assert!(kinds.contains(&TokenKind::Float(3.0)));
    }

    #[test]
    fn else_on_next_line_does_not_split() {
        let mut lexer = Lexer::new("if x {\n} else {\n}\n");
        let tokens = lexer.tokenize().unwrap();
        let kinds: Vec<TokenKind> = tokens.into_iter().map(|t| t.kind).collect();
        assert!(kinds.contains(&TokenKind::Else));
        assert_eq!(kinds.iter().filter(|k| **k == TokenKind::Semicolon).count(), 1);
    }
}