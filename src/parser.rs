//! Parser for turning tokenized `.fitlog` files into a series of directives
//! that will be used for populating the in-memory data model.

use logos::Logos;

use crate::{
    directives::{
        ActivityDecl, ActivityHeader,
        Directive::{self},
        Entry, ExerciseDecl, ExerciseSlotKind, Ident, Include, MetaItem, MetricAliasDecl,
        MetricDecl, RecordLine, RecordSegment, RecordValue, RecordValueKind, Span,
    },
    lexer::{
        Token::{self, Newline},
        token_name,
    },
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

    fn current_start(&self) -> usize {
        if self.at_eof() {
            self.prev_end()
        } else {
            self.peek().expect("Not at EOF").1.start
        }
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

    /// Parses a single record value that can appear as part of an exercise or
    /// a measurement. Record values can have one or more numbers
    /// (slash-delimited) and an optional unit.
    /// Ex. `12 lbs`
    /// Ex. `12/12/14`
    /// Ex. `3/4/5 mi`
    fn parse_entry_record_value(&mut self) -> PResult<RecordValue> {
        let mut values = vec![];
        // There is at least one number value
        loop {
            let value = self.number()?;
            values.push(value);
            // multiple number values are separated by slashes
            if !self.check(Token::ForwardSlash) {
                break;
            }
            self.eat(Token::ForwardSlash);
        }
        // Optionally, there is a unit after the number(s)
        let unit = if self.check(Token::Word) {
            Some(self.word()?)
        } else {
            None
        };
        Ok(RecordValue {
            value: if values.len() == 1 {
                RecordValueKind::Single(values[0].0)
            } else {
                RecordValueKind::List(values.iter().map(|(v, _)| v.clone()).collect())
            },
            unit: unit.clone(),
            span: if unit.is_some() {
                values[0].1.start..unit.unwrap().span.end
            } else {
                values.first().unwrap().1.start..values.last().unwrap().1.end
            },
        })
    }

    /// Parses an entry record segment. These belong to a RecordLine.
    fn parse_entry_record_segment(&mut self, name: Option<Ident>) -> PResult<RecordSegment> {
        let start = match &name {
            Some(n) => n.span.start,
            None => self.current_start(),
        };
        let mut values = vec![];
        while self.check(Token::Number) {
            values.push(self.parse_entry_record_value()?);
        }
        if values.is_empty() {
            return Err(ParseError {
                msg: "Expected a value".into(),
                span: self.current_start()..self.current_start(),
            });
        }
        Ok(RecordSegment {
            name,
            values,
            span: start..self.prev_end(),
        })
    }

    fn parse_record_line_starting_with(&mut self, name: Ident) -> PResult<RecordLine> {
        let start = name.span.start;
        let mut segments = vec![self.parse_entry_record_segment(None)?];
        while self.eat(Token::Comma) {
            let segment_name = if self.check(Token::Word) {
                Some(self.word()?)
            } else {
                None
            };
            segments.push(self.parse_entry_record_segment(segment_name)?);
        }
        Ok(RecordLine {
            name,
            segments,
            span: start..self.prev_end(),
        })
    }

    fn parse_record_line(&mut self) -> PResult<RecordLine> {
        let name = self.word()?;
        self.parse_record_line_starting_with(name)
    }

    fn parse_tags(&mut self) -> Vec<String> {
        let mut tags = vec![];
        loop {
            if !self.check(Token::Tag) {
                break;
            }
            let tag = self
                .expect(Token::Tag)
                .expect("already checked against tag");
            let tag_string = self.slice(&tag)[1..].to_string();
            tags.push(tag_string);
        }
        tags
    }

    fn parse_metadata_item(&mut self) -> PResult<MetaItem> {
        let key = self.word()?;
        self.expect(Token::Colon)?;
        let value_span = self.expect(Token::Str)?;
        let value = self.slice(&value_span);
        Ok(MetaItem {
            key: key.clone(),
            value: value.to_string(),
            span: key.span.start..value_span.end,
        })
    }

    /// Parse an Entry directive. Always begins with a date and optionally a
    /// time, then the data recorded at that date/time (Metrics or Activity).
    fn parse_entry(&mut self) -> PResult<Directive> {
        let start = self.current_start();
        let date_span = self.expect(Token::Date)?;
        let date = match parse_date(self.slice(&date_span)) {
            Ok(d) => d,
            Err(e) => {
                return Err(ParseError {
                    msg: e,
                    span: date_span,
                });
            }
        };

        let time = if self.check(Token::Time) {
            let time_span = self.expect(Token::Time)?;
            let time_string = self.slice(&time_span);
            let Some(time) = parse_time(time_string) else {
                return Err(ParseError {
                    msg: format!(
                        "`{}` is not a valid time (expected 00:00-23:59)",
                        time_string
                    ),
                    span: time_span,
                });
            };
            Some(time)
        } else {
            None
        };

        let name = self.word()?;
        let mut activity_header: Option<ActivityHeader> = None;
        let mut records = vec![];
        if self.check(Token::Number) {
            // name followed by number means this is actually a record line
            records.push(self.parse_record_line_starting_with(name)?);
        } else {
            // name followed by non-number means this is likely an exercise
            let description = if self.check(Token::Str) {
                Some(self.string()?.0)
            } else {
                None
            };
            activity_header = Some(ActivityHeader {
                span: name.span.start..self.prev_end(),
                name,
                description,
            });
        }

        let tags = self.parse_tags();
        let mut metadata = vec![];

        // Additional lines
        loop {
            if self.at_eof() {
                break;
            }
            if !self.at_newline() {
                return Err(ParseError {
                    msg: "Unexpected token after entry".into(),
                    span: self.current_start()..self.prev_end(),
                });
            }
            if !self.newline_is_indented() {
                // a non-indented new-line means this entry is done
                self.eat_newline();
                break;
            }
            self.eat_newline();
            if self.at_newline() {
                continue; // skip the empty newline and try again
            }

            // Additional lines after the first entry line can only be one of
            // two things:
            // 1. a record (can be a measurement or an exercise) (ex.
            //    `weight 165 lb` or `bench_press 185 lb 5/5/4`)
            // 2. metadata (ex. `key: "Value"`)
            match (self.peek(), self.peek2()) {
                (Some((Ok(Token::Word), _)), Some((Ok(Token::Colon), _))) => {
                    metadata.push(self.parse_metadata_item()?);
                }
                (Some((Ok(Token::Word), _)), _) => {
                    records.push(self.parse_record_line()?);
                }
                (Some(_), _) => {
                    // Found some other kind of token
                    return Err(ParseError {
                        msg: "Expected a measurement, exercise, or `key:` metadata".into(),
                        span: self.current_start()..self.prev_end(),
                    });
                }
                (None, _) => {
                    // Found no tokens: end-of-file
                    break;
                }
            }

            // Any other remaining tokens before the eof/newline is an error
            if !self.at_eof() && !self.at_newline() {
                return Err(ParseError {
                    msg: "Unexpected token after record line".into(),
                    span: self.current_start()..self.prev_end(),
                });
            }
        }

        Ok(Directive::Entry(Entry {
            date,
            time,
            activity: activity_header,
            records,
            tags,
            metadata,
            span: start..self.prev_end(),
        }))
    }

    /// Parse a pragma directive - currently just `!include "path/to/file"`
    fn parse_pragma(&mut self) -> PResult<Directive> {
        // Caller has already confirmed that this directive starts with `!`
        let bang = self.expect(Token::Bang)?;
        let word = self.word()?;

        // Currently, the only pragma directive is "include"
        if word.text != "include" {
            return Err(ParseError {
                msg: format!(
                    "Unknown pragma `{}`. Only `include` is supported.",
                    word.text
                ),
                span: word.span.clone(),
            });
        }

        let (path, _span) = self.string()?;
        Ok(Directive::Include(Include {
            path: path,
            span: bang.start..self.prev_end(),
        }))
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

    /// Parse an activity declaration directive
    /// Syntax: `activity <name>`
    fn parse_activity_decl(&mut self) -> PResult<Directive> {
        // Caller has already confirmed that the word is "activity"
        let keyword = self.expect(Token::Word)?;
        let name = self.word()?;
        let span = keyword.start..self.prev_end();
        self.end_of_directive()?;
        Ok(Directive::Activity(ActivityDecl { name, span }))
    }

    /// Parse an activity declaration directive
    /// Syntax: `exercise <name> <slot> [slot...]`
    fn parse_exercise_decl(&mut self) -> PResult<Directive> {
        // Caller has already confirmed that the word is "exercise"
        let keyword = self.expect(Token::Word)?;
        let name = self.word()?;
        let mut slots: Vec<ExerciseSlotKind> = vec![];
        // Parse all slots
        loop {
            if !self.check(Token::Word) {
                break;
            }
            let slot_token = self.word()?;
            let slot_kind = match slot_token.text.as_str() {
                "load" => ExerciseSlotKind::Load,
                "reps" => ExerciseSlotKind::Reps,
                "duration" => ExerciseSlotKind::Distance,
                "distance" => ExerciseSlotKind::Distance,
                other => {
                    return Err(ParseError {
                        msg: format!(
                            "Unexpected slot kind. Expected `load`, `reps`, `duration`, `distance`. Found `{}`.",
                            other
                        ),
                        span: slot_token.span,
                    });
                }
            };
            slots.push(slot_kind);
        }

        if slots.len() <= 0 {
            return Err(ParseError {
                msg: format!(
                    "Expected at least one slot declaration for exercise `{}`.",
                    name.text
                ),
                span: keyword.start..self.prev_end(),
            });
        }

        let span = keyword.start..self.prev_end();
        self.end_of_directive()?;

        Ok(Directive::Exercise(ExerciseDecl { name, slots, span }))
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

fn days_in_month(y: u16, m: u8) -> u8 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(y) => 29,
        2 => 28,
        _ => unreachable!("month validated before call"),
    }
}

fn is_leap_year(y: u16) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn parse_time(time_str: &str) -> Option<(u8, u8)> {
    let (h, m) = time_str
        .split_once(':')
        .expect("Time token always contains ':'");
    let h: u8 = h.parse().expect("two hour digits fit in u8");
    let m: u8 = m.parse().expect("two minute digits fit u8");

    if h > 23 || m > 59 {
        return None;
    }

    Some((h, m))
}

fn parse_date(date_string: &str) -> Result<(u16, u8, u8), String> {
    let (y, rest) = date_string
        .split_once('-')
        .expect("Date token contains '-'");
    let (m, d) = rest
        .split_once('-')
        .expect("Date token contains second '-'");
    let y: u16 = y.parse().expect("Four year digits fit in u16");
    let m: u8 = m.parse().expect("Two month digits fit in u8");
    let d: u8 = d.parse().expect("Two day digits fit in u8");

    if m < 1 || m > 12 {
        return Err(format!("`{}` is not a valid month (expected 1-12)", m));
    }

    if d < 1 || d > days_in_month(y, m) {
        return Err(format!(
            "`{}` is not a valid day for the specified month.",
            d
        ));
    }

    Ok((y, m, d))
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

#[cfg(test)]
mod tests {
    use log::Record;

    use crate::{
        directives::{Directive, ExerciseSlotKind, RecordValueKind},
        lexer::{Token, token_name},
        parser::{ParseError, parse},
    };

    fn parse_ok(src: &str) -> Vec<Directive> {
        let (dirs, errs) = parse(src);
        assert!(errs.is_empty(), "unexpected errors: {errs:#?}");
        dirs
    }

    fn parse_errs(src: &str) -> Vec<ParseError> {
        parse(src).1
    }

    #[test]
    fn empty_file() {
        let dirs = parse_ok("");
        assert_eq!(dirs.len(), 0);
    }

    #[test]
    fn comment_only_file() {
        let dirs = parse_ok(
            r";this is a test

; to make sure files that ; are just comments
; parse successfully
",
        );
        assert_eq!(dirs.len(), 0);
    }

    #[test]
    fn basic_metric() {
        let dirs = parse_ok("metric weight lb");
        let [Directive::Metric(m)] = &dirs[..] else {
            panic!("expected one metric, got {dirs:#?}")
        };
        assert_eq!(m.name.text, "weight");
        assert_eq!(m.unit.text, "lb");
        assert!(!m.is_additive);
    }

    #[test]
    fn metric_with_percent_unit() {
        let dirs = parse_ok("metric bodyfat %");
        let [Directive::Metric(m)] = &dirs[..] else {
            panic!("expected one metric, got {dirs:#?}")
        };
        assert_eq!(m.name.text, "bodyfat");
        assert_eq!(m.unit.text, "%");
        assert!(!m.is_additive);
    }

    #[test]
    fn metric_with_additive() {
        let dirs = parse_ok("metric steps count additive");
        let [Directive::Metric(m)] = &dirs[..] else {
            panic!("expected one metric, got {dirs:#?}")
        };
        assert_eq!(m.name.text, "steps");
        assert_eq!(m.unit.text, "count");
        assert!(m.is_additive);
    }

    #[test]
    fn metric_with_hyphen_name() {
        let dirs = parse_ok("metric dance-time min additive");
        let [Directive::Metric(m)] = &dirs[..] else {
            panic!("expected one metric, got {dirs:#?}")
        };
        assert_eq!(m.name.text, "dance-time");
        assert_eq!(m.unit.text, "min");
        assert!(m.is_additive);
    }

    #[test]
    fn metric_alias() {
        let dirs = parse_ok("metric bp = bp_sys / bp_dia");
        let [Directive::MetricAlias(m)] = &dirs[..] else {
            panic!("expected one metric alias, got {dirs:#?}")
        };
        assert_eq!(m.name.text, "bp");
        assert_eq!(m.composed_metric_names.get(0).unwrap().text, "bp_sys");
        assert_eq!(m.composed_metric_names.get(1).unwrap().text, "bp_dia");
    }

    #[test]
    fn metric_alias_four() {
        let dirs = parse_ok("metric crazy = one / two / three / four");
        let [Directive::MetricAlias(m)] = &dirs[..] else {
            panic!("expected one metric alias, got {dirs:#?}")
        };
        assert_eq!(m.name.text, "crazy");
        assert_eq!(m.composed_metric_names.get(0).unwrap().text, "one");
        assert_eq!(m.composed_metric_names.get(1).unwrap().text, "two");
        assert_eq!(m.composed_metric_names.get(2).unwrap().text, "three");
        assert_eq!(m.composed_metric_names.get(3).unwrap().text, "four");
    }

    #[test]
    fn incomplete_metric() {
        let errs = parse_errs("metric");
        let [error] = &errs[..] else {
            panic!("expected one parse error, got {errs:#?}")
        };
        assert!(error.msg.contains(token_name(&Token::Word)));
    }

    #[test]
    fn incomplete_metric_unit() {
        let errs = parse_errs("metric weight");
        let [error] = &errs[..] else {
            panic!("expected one parse error, got {errs:#?}")
        };
        assert!(error.msg.contains(token_name(&Token::Word)));
    }

    #[test]
    fn extra_metric_words() {
        let errs = parse_errs("metric weight lb extra");
        let [error] = &errs[..] else {
            panic!("expected one parse error, got {errs:#?}")
        };
        assert!(error.msg.contains("Unexpected"));
    }

    #[test]
    fn basic_activity() {
        let dirs = parse_ok("activity hike");
        let [Directive::Activity(a)] = &dirs[..] else {
            panic!("expected one activity, got {dirs:#?}")
        };
        assert_eq!(a.name.text, "hike");
    }

    #[test]
    fn extra_activity_words() {
        let errs = parse_errs("activity run pooplol");
        let [error] = &errs[..] else {
            panic!("expected one parse error, got {errs:#?}")
        };
        assert!(error.msg.contains("Unexpected"));
    }

    #[test]
    fn basic_exercise_decl() {
        let dirs = parse_ok("exercise pull_up reps");
        let [Directive::Exercise(e)] = &dirs[..] else {
            panic!("expected one exercise decl, got {dirs:#?}")
        };
        assert_eq!(e.name.text, "pull_up");
        assert_eq!(e.slots, [ExerciseSlotKind::Reps]);
    }

    #[test]
    fn multiple_slot_exercise_decl() {
        let dirs = parse_ok("exercise dumbbell_curl load reps");
        let [Directive::Exercise(e)] = &dirs[..] else {
            panic!("expected one exercise decl, got {dirs:#?}")
        };
        assert_eq!(e.name.text, "dumbbell_curl");
        assert_eq!(e.slots, [ExerciseSlotKind::Load, ExerciseSlotKind::Reps]);
    }

    #[test]
    fn no_slot_exercise_decl() {
        let errs = parse_errs("exercise dancing");
        let [error] = &errs[..] else {
            panic!("expected one parse error, got {errs:#?}")
        };
        assert!(error.msg.contains("slot"));
    }

    #[test]
    fn extra_words_exercise_decl() {
        let errs = parse_errs("exercise dancing duration pooplol");
        let [error] = &errs[..] else {
            panic!("expected one parse error, got {errs:#?}")
        };
        assert!(error.msg.contains("Unexpected"));
    }

    #[test]
    fn basic_include() {
        let dirs = parse_ok("!include \"test_file.fitlog\"");
        let [Directive::Include(i)] = &dirs[..] else {
            panic!("expected one include directive, got {dirs:#?}")
        };
        assert_eq!(i.path, "test_file.fitlog");
    }

    #[test]
    fn absolute_path_include() {
        let dirs = parse_ok("!include \"C:\\Hayden\\Documents\\Fitness Data\\test_file.fitlog\"");
        let [Directive::Include(i)] = &dirs[..] else {
            panic!("expected one include directive, got {dirs:#?}")
        };
        assert_eq!(
            i.path,
            "C:\\Hayden\\Documents\\Fitness Data\\test_file.fitlog"
        );
    }

    #[test]
    fn empty_include() {
        let errs = parse_errs("!include");
        let [error] = &errs[..] else {
            panic!("expected one parse error, got {errs:#?}")
        };
        assert!(error.msg.contains("Expected a \"string\""));
    }

    #[test]
    fn unknown_directive() {
        let errs = parse_errs("pooplol");
        let [error] = &errs[..] else {
            panic!("expected one parse error, got {errs:#?}")
        };
        assert!(error.msg.contains("Unknown directive"));
    }

    #[test]
    fn basic_metric_entry() {
        let dirs = parse_ok("2026-08-31 22:39 weight 139 lb");
        let [Directive::Entry(e)] = &dirs[..] else {
            panic!("expected one entry directive, got {dirs:#?}")
        };
        assert_eq!(e.date, (2026, 8, 31));
        assert_eq!(e.time, Some((22, 39)));
        assert_eq!(e.activity, None);
        assert_eq!(e.records[0].name.text, "weight");
        assert_eq!(
            e.records[0].segments[0].values[0].value,
            RecordValueKind::Single(139.0)
        );
        assert_eq!(
            e.records[0].segments[0].values[0]
                .unit
                .clone()
                .unwrap()
                .text,
            "lb"
        );
        assert_eq!(e.metadata.len(), 0);
    }

    #[test]
    fn compound_metric_entry() {
        let dirs = parse_ok("2026-08-31 01:23 bp 100/60");
        let [Directive::Entry(e)] = &dirs[..] else {
            panic!("expected one entry directive, got {dirs:#?}")
        };
        assert_eq!(e.date, (2026, 8, 31));
        assert_eq!(e.time, Some((1, 23)));
        assert_eq!(e.activity, None);
        assert_eq!(e.records[0].name.text, "bp");
        assert_eq!(
            e.records[0].segments[0].values[0].value,
            RecordValueKind::List(vec![100.0, 60.0])
        );
        assert_eq!(e.records[0].segments[0].values[0].unit, None);
        assert_eq!(e.metadata.len(), 0);
    }

    #[test]
    fn multiple_metric_same_line_entry() {
        let dirs = parse_ok("2026-08-31 01:23 bp 100/60, weight 180 lb");
        let [Directive::Entry(e)] = &dirs[..] else {
            panic!("expected one entry directive, got {dirs:#?}")
        };
        assert_eq!(e.date, (2026, 8, 31));
        assert_eq!(e.time, Some((1, 23)));
        assert_eq!(e.activity, None);
        assert_eq!(e.records[0].name.text, "bp");
        assert_eq!(e.records[0].segments[0].name, None);
        assert_eq!(
            e.records[0].segments[0].values[0].value,
            RecordValueKind::List(vec![100.0, 60.0])
        );
        assert_eq!(e.records[0].segments[0].values[0].unit, None);
        assert_eq!(
            e.records[0].segments[1].name.clone().unwrap().text,
            "weight"
        );
        assert_eq!(
            e.records[0].segments[1].values[0].value,
            RecordValueKind::Single(180.0)
        );
        assert_eq!(
            e.records[0].segments[1].values[0]
                .unit
                .clone()
                .unwrap()
                .text,
            "lb"
        );
        assert_eq!(e.metadata.len(), 0);
    }

    #[test]
    fn multiple_metric_multiple_line_entry() {
        let dirs = parse_ok("2026-08-31 01:23 bp 100/60\n\tweight 180 lb\n\tbodyfat 18.2%");
        let [Directive::Entry(e)] = &dirs[..] else {
            panic!("expected one entry directive, got {dirs:#?}")
        };
        assert_eq!(e.date, (2026, 8, 31));
        assert_eq!(e.time, Some((1, 23)));
        assert_eq!(e.records[0].name.text, "bp");
        assert_eq!(
            e.records[0].segments[0].values[0].value,
            RecordValueKind::List(vec![100.0, 60.0])
        );
        assert_eq!(e.records[1].name.text, "weight");
        assert_eq!(
            e.records[1].segments[0].values[0].value,
            RecordValueKind::Single(180.0)
        );
        assert_eq!(
            e.records[1].segments[0].values[0]
                .unit
                .clone()
                .unwrap()
                .text,
            "lb"
        );
        assert_eq!(e.records[2].name.text, "bodyfat");
        assert_eq!(
            e.records[2].segments[0].values[0].value,
            RecordValueKind::Single(18.2)
        );
        assert_eq!(
            e.records[2].segments[0].values[0]
                .unit
                .clone()
                .unwrap()
                .text,
            "%"
        );
    }
}
