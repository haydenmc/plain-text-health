//! Parser for turning tokenized `.fitlog` files into a series of directives
//! that will be used for populating the in-memory data model.

use logos::Logos;

use crate::{
    directives::{Directive, Ident, MetricAliasDecl, MetricDecl, Span},
    lexer::{Token, token_name},
};

type Lexed = (Result<Token, ()>, Span);

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub msg: String,
    pub span: Span,
}

type PResult<T> = Result<T, ParseError>;

pub struct Parser<'src> {
    src: &'src str,
    toks: Vec<(Result<Token, ()>, Span)>,
    pos: usize,
    errors: Vec<ParseError>,
}

impl<'src> Parser<'src> {
    /// Returns the current token.
    fn peek(&self) -> Option<&Lexed> {
        self.toks.get(self.pos)
    }

    /// Returns the token beyond the current token.
    fn peek2(&self) -> Option<&Lexed> {
        self.toks.get(self.pos + 1)
    }

    /// Advances the cursor to the next token.
    fn advance(&mut self) -> Option<(Result<Token, ()>, Span)> {
        let t = self.toks.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    /// Returns the string value of the given span range.
    fn slice(&self, span: &Span) -> &'src str {
        &self.src[span.clone()]
    }

    /// Returns true if the current token is the last in the file.
    fn at_eof(&self) -> bool {
        self.pos >= self.toks.len()
    }

    /// Returns the end-offset of the previous token.
    fn prev_end(&self) -> usize {
        if self.pos == 0 {
            return 0;
        }
        self.toks[self.pos - 1].1.end
    }

    /// Returns true if the current token matches the given token. Returns false
    /// otherwise.
    fn check(&self, want: Token) -> bool {
        match self.peek() {
            Some((Ok(token), _)) => *token == want,
            _ => false,
        }
    }

    /// If the current token matches the given token, the cursor is advanced and
    /// true is returned.
    /// False is returned otherwise.
    fn eat(&mut self, want: Token) -> bool {
        if !self.check(want) {
            return false;
        }
        self.pos += 1;
        true
    }

    /// If the current token is a word that matches the given string, the cursor
    /// is advanced and true is returned.
    /// Returns false otherwise.
    fn eat_word(&mut self, text: &str) -> bool {
        let Some((Ok(Token::Word), span)) = self.peek() else {
            return false;
        };
        if self.slice(span) != text {
            return false;
        }
        self.pos += 1;
        true
    }

    /// Returns the string value of the current token.
    fn peek_slice(&self) -> Option<&'src str> {
        self.toks.get(self.pos).map(|(_, sp)| self.slice(sp))
    }

    /// If the current token matches the `want` token, the cursor is advanced,
    /// and the matching span is returned.
    /// Otherwise, an error is returned.
    fn expect(&mut self, want: Token) -> PResult<Span> {
        match self.peek() {
            Some((Ok(tok), span)) if *tok == want => {
                let span = span.clone();
                self.pos += 1;
                Ok(span)
            }
            Some((_, span)) => Err(ParseError {
                msg: format!(
                    "Expected {}, found `{}`",
                    token_name(&want),
                    self.slice(span)
                ),
                span: span.clone(),
            }),
            None => Err(ParseError {
                msg: format!("Expected {}, found end of file", token_name(&want)),
                span: self.prev_end()..self.prev_end(),
            }),
        }
    }

    /// If the current token is a Word, the cursor is advanced, and the
    /// value/span of the Word is returned.
    /// Otherwise, an error is returned.
    fn word(&mut self) -> PResult<Ident> {
        let span = self.expect(Token::Word)?;
        Ok(Ident {
            text: self.slice(&span).to_string(),
            span: span,
        })
    }

    /// If the current token is a number, the cursor is advanced, and the
    /// value/span of the number is returned.
    /// Otherwise, an error is returned.
    fn number(&mut self) -> PResult<(f64, Span)> {
        let span = self.expect(Token::Number)?;
        let num: f64 = self.slice(&span).parse().map_err(|_| ParseError {
            msg: format!("`{}` could not be parsed as a number", self.slice(&span)),
            span: span.clone(),
        })?;
        Ok((num, span))
    }

    /// If the current token is a string, the cursor is advanced, and the string
    /// plus the span is returned.
    /// Otherwise, an error is returned.
    fn string(&mut self) -> PResult<(String, Span)> {
        let span = self.expect(Token::Str)?;
        let slice = self.slice(&span);
        Ok((slice[1..slice.len() - 1].to_string(), span))
    }

    /// Returns true if the current token is a new line. Returns false
    /// otherwise.
    fn at_newline(&self) -> bool {
        self.check(Token::Newline)
    }

    /// Returns true if the current token is a new line followed by any kind of
    /// indent characters (spaces/tabs).
    fn newline_is_indented(&self) -> bool {
        if !self.at_newline() {
            return false;
        }
        let Some(s) = self.peek_slice() else {
            return false;
        };
        // line break can either be \r\n or just \n.
        // make sure we properly handle both.
        if s.starts_with('\r') {
            s.len() > 2
        } else {
            s.len() > 1
        }
    }

    /// Advances the cursor if the current token is a new line.
    fn eat_newline(&mut self) -> bool {
        self.eat(Token::Newline)
    }

    /// Advances the cursor through every consecutive new line token.
    fn skip_blank_lines(&mut self) {
        while self.eat_newline() {}
    }

    /// Skip to the start of the next top-level (unindented) line, or EOF.
    /// Helps to recover from errors and other bad state that might prevent us
    /// from continuing to parse successfully.
    fn synchronize(&mut self) {
        loop {
            if self.at_eof() {
                return;
            }
            if self.at_newline() && !self.newline_is_indented() {
                self.eat_newline();
                return;
            }
            self.advance();
        }
    }

    /// Directive must end here: newline (consumed) or EOF.
    /// Trailing tokens are an error.
    fn end_of_directive(&mut self) -> PResult<()> {
        if self.at_eof() || self.eat_newline() {
            return Ok(());
        }
        let sp = self.peek().map(|(_, s)| s.clone()).unwrap();
        Err(ParseError {
            msg: format!("Unexpected `{}` after directive", self.slice(&sp)),
            span: sp,
        })
    }

    fn parse_entry(&mut self) -> PResult<Directive> {
        todo!()
    }

    fn parse_pragma(&mut self) -> PResult<Directive> {
        todo!()
    }

    /// Parse a matric declaration directive
    /// Syntax: `metric <name> <unit> [additive]`
    fn parse_metric_decl(&mut self) -> PResult<Directive> {
        // Caller has already confirmed that the word is "metric"
        let keyword = self.expect(Token::Word)?;
        let name = self.word()?;

        // Handle compound metrics
        // Syntax: `metric <name> = <metric_a> / <metric_b> [/ <metric_c> ...]`
        if self.eat(Token::Equals) {
            let mut components = vec![self.word()?];
            while self.eat(Token::ForwardSlash) {
                components.push(self.word()?);
            }
            let span = keyword.start..self.prev_end();
            self.end_of_directive()?;
            return Ok(Directive::MetricAlias(MetricAliasDecl {
                name,
                composed_metric_names: components,
                span,
            }));
        }

        // Non-compound metrics
        let unit = self.word()?;
        let is_additive = self.eat_word("additive");
        let span = keyword.start..self.prev_end();
        self.end_of_directive()?;
        Ok(Directive::Metric(MetricDecl {
            name,
            unit,
            is_additive,
            span,
        }))
    }

    fn parse_activity_decl(&mut self) -> PResult<Directive> {
        todo!()
    }

    fn parse_exercise_decl(&mut self) -> PResult<Directive> {
        todo!()
    }

    fn parse_directive(&mut self) -> PResult<Directive> {
        match self.peek() {
            // Lines beginning with dates always indicate an Entry Directive
            Some((Ok(Token::Date), _)) => self.parse_entry(),
            // Lines beginning with a bang `!` always indicate a Pragma Directive
            Some((Ok(Token::Bang), _)) => self.parse_pragma(),
            // Lines beginning with a word could be one of several directives
            Some((Ok(Token::Word), sp)) => {
                let sp = sp.clone();
                match self.slice(&sp) {
                    "metric" => self.parse_metric_decl(),
                    "activity" => self.parse_activity_decl(),
                    "exercise" => self.parse_exercise_decl(),
                    other => Err(ParseError {
                        msg: format!(
                            "Unknown directive `{other}` - expected `metric`, `activity`, `exercise`, or a dated entry."
                        ),
                        span: sp,
                    }),
                }
            }
            // Invalid directive token
            Some((_, sp)) => Err(ParseError {
                msg: "Expected a directive".into(),
                span: sp.clone(),
            }),
            None => unreachable!("callers should ensure at_eof is false"),
        }
    }

    fn run(mut self) -> (Vec<Directive>, Vec<ParseError>) {
        let mut out = Vec::new();
        loop {
            self.skip_blank_lines();
            if self.at_eof() {
                break;
            }
            match self.parse_directive() {
                Ok(d) => out.push(d),
                Err(e) => {
                    self.errors.push(e);
                    self.synchronize();
                }
            }
        }
        (out, self.errors)
    }
}

pub fn parse(src: &str) -> (Vec<Directive>, Vec<ParseError>) {
    let toks = Token::lexer(src)
        .spanned()
        // Comments are filtered out here before they reach parser logic
        .filter(|(t, _)| !matches!(t, Ok(Token::Comment)))
        .collect();
    Parser {
        src,
        toks,
        pos: 0,
        errors: Vec::new(),
    }
    .run()
}
