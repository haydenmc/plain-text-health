//! Lexer for the plain-text-health `.fitlog` files
//!
//! Contains functions for tokenizing the source text

use logos::Logos;

/// Represents a single lexer token
#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t]+")]
enum Token {
    // Literals:
    /// Calendar date YYYY-MM-DD
    #[regex(r"\d{4}-\d{2}-\d{2}")]
    Date,
    /// HH:MM time
    #[regex(r"\d{2}:\d{2}")]
    Time,
    /// Any positive/negative number with optional decimal places
    #[regex(r"-?\d+(\.\d+)?", |lex| lex.slice().parse::<f64>().ok())]
    Number(f64),
    /// Quoted string
    #[regex(r#""[^"\n]*""#)]
    Str,
    /// Names, keywords, units (unquoted), % (special for percentage unit)
    #[regex(r"[A-Za-z_][A-Za-z0-9_-]*|%")]
    Word,
    /// Tags
    #[regex(r"#[A-Za-z0-9][A-Za-z0-9_-]*")]
    Tag,

    // Punctuation:
    #[token(",")]
    Comma,
    #[token(":")]
    Colon,
    #[token("/")]
    Slash,
    #[token("=")]
    Equals,
    #[token("..")]
    DotDot,

    // Structure:
    #[regex(r"\r?\n[ \t]*")]
    Newline,
    #[regex(r";[^\n]*", allow_greedy = true)]
    Comment,
}

#[cfg(test)]
mod tests {
    use super::*;
    use logos::Logos;

    fn lex_tokens(src: &str) -> Vec<Token> {
        Token::lexer(src)
            .spanned()
            .map(|(tok, span)| match tok {
                Ok(t) => t,
                Err(_) => panic!(
                    "Unexpected lex error at {:?}: {:?}",
                    span.clone(),
                    &src[span]
                ),
            })
            .collect()
    }

    #[test]
    fn lex_date() {
        assert_eq!(lex_tokens("2026-08-05")[0], Token::Date);
    }

    #[test]
    fn lex_date_time() {
        assert_eq!(lex_tokens("2026-08-05 15:01"), [Token::Date, Token::Time]);
    }

    #[test]
    fn lex_metric_definition() {
        assert_eq!(
            lex_tokens("metric weight lb range: 50..500"),
            [
                Token::Word,
                Token::Word,
                Token::Word,
                Token::Word,
                Token::Colon,
                Token::Number(50.0),
                Token::DotDot,
                Token::Number(500.0),
            ]
        )
    }

    #[test]
    fn lex_metric_alias() {
        use Token::*;
        assert_eq!(
            lex_tokens("metric bp = bp_sys / bp_dia"),
            [Word, Word, Equals, Word, Slash, Word]
        )
    }

    #[test]
    fn lex_multi_line_metric_record() {
        use Token::*;
        assert_eq!(
            lex_tokens(
                "2026-08-05 08:12 weight 178.4 lb, bodyfat 18.2 %\n  document: \"assets/2026/2026-08-05-progress.jpg\""
            ),
            [
                Date,
                Time,
                Word,
                Number(178.4),
                Word,
                Comma,
                Word,
                Number(18.2),
                Word,
                Newline,
                Word,
                Colon,
                Str
            ]
        )
    }

    #[test]
    fn lex_activity_definition() {
        assert_eq!(lex_tokens("activity hike"), [Token::Word, Token::Word]);
    }

    #[test]
    fn lex_simple_activity_record() {
        use Token::*;
        let activity_source = "2026-08-06 13:29 hike \"Little Si Afternoon Hike\" #pnw #summer";
        assert_eq!(
            lex_tokens(activity_source),
            [Date, Time, Word, Str, Tag, Tag]
        );
    }

    #[test]
    fn lex_multi_line_activity_record() {
        use Token::*;
        assert_eq!(
            lex_tokens(
                r#"2026-08-05 hike "Cougar Mountain loop" #pnw #summer
  duration 2.5 h, distance 6.1 mi
  avg_hr 148 bpm
  document: "assets/2026/gpx/2026-08-05-cougar.gpx""#
            ),
            [
                Date,
                Word,
                Str,
                Tag,
                Tag,
                Newline,
                Word,
                Number(2.5),
                Word,
                Comma,
                Word,
                Number(6.1),
                Word,
                Newline,
                Word,
                Number(148.0),
                Word,
                Newline,
                Word,
                Colon,
                Str
            ]
        );
    }
}
