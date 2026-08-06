//! Parser for the plain-text-health `.pth` files
//! 
//! Contains functions for tokenizing the source text and then parsing the token
//! sequences into valid plain-text-health statements.

use logos::Logos;

/// Represents a single lexer token expected from `.pth` files
#[derive(Logos, Debug, PartialEq)]
#[logos(skip r"[ \t\n\f]+")]
enum Token {
    /// Calendar date YYYY-MM-DD
    #[regex(r"\d{4}-\d{2}-\d{2}")] Date,
    /// Any positive/negative number with optional decimal places
    #[regex(r"-?\d+(\.\d+)?")]     Number,
}

// #[derive(Logos)]
// enum Token {
//     #[regex(r"\d{4}-\d{2}-\d{2}")]  Date,
//     #[regex(r"\d{2}:\d{2}")]        Time,
//     #[regex(r"-?\d+(\.\d+)?")]      Number,
//     #[regex(r"#[\w-]+")]            Tag,
//     #[regex(r"\^[\w-]+")]           Link,
//     #[regex(r#""[^"]*""#)]          String,
//     #[regex(r"[A-Za-z][\w/%]*")]    Word,   // types, units, metadata keys
//     #[token(":")]                    Colon,
// }