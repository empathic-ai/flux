use core::{
    fmt::{self, Write},
    num::ParseIntError,
    str::from_utf8_unchecked,
};

use bevy::{prelude::*};

use derive_more::derive::{Display, Error, From};

use super::{Access, ReflectPathError};

/// An error that occurs when parsing reflect path strings.
#[derive(Debug, PartialEq, Eq, Error, Display)]
#[error(ignore)]
pub struct ParseError<'a>(Error<'a>);

/// A parse error for a path string.
#[derive(Debug, PartialEq, Eq, Error, Display, From)]
enum Error<'a> {
    #[display("expected an identifier, but reached end of path string")]
    NoIdent,

    #[display("expected an identifier, got '{_0}' instead")]
    #[error(ignore)]
    #[from(ignore)]
    ExpectedIdent(Token<'a>),

    #[display("failed to parse index as integer")]
    InvalidIndex(ParseIntError),

    #[display("a '[' wasn't closed, reached end of path string before finding a ']'")]
    Unclosed,

    #[display("a '[' wasn't closed properly, got '{_0}' instead")]
    #[error(ignore)]
    #[from(ignore)]
    BadClose(Token<'a>),

    #[display("a ']' was found before an opening '['")]
    CloseBeforeOpen,
}

pub(super) struct PathParser<'a> {
    path: &'a str,
    remaining: &'a [u8],
}
impl<'a> PathParser<'a> {
    pub(super) fn new(path: &'a str) -> Self {
        let remaining = path.as_bytes();
        PathParser { path, remaining }
    }

    fn next_token(&mut self) -> Option<Token<'a>> {
        let to_parse = self.remaining;

        // Return with `None` if empty.
        let (first_byte, remaining) = to_parse.split_first()?;

        if let Some(token) = Token::symbol_from_byte(*first_byte) {
            self.remaining = remaining; // NOTE: all symbols are ASCII
            return Some(token);
        }
        // We are parsing either `0123` or `field`.
        // If we do not find a subsequent token, we are at the end of the parse string.
        let ident_len = to_parse.iter().position(|t| Token::SYMBOLS.contains(t));
        let (ident, remaining) = to_parse.split_at(ident_len.unwrap_or(to_parse.len()));
        // SAFETY: This relies on `self.remaining` always remaining valid UTF8:
        // - self.remaining is a slice derived from self.path (valid &str)
        // - The slice's end is either the same as the valid &str or
        //   the last byte before an ASCII utf-8 character (ie: it is a char
        //   boundary).
        // - The slice always starts after a symbol ie: an ASCII character's boundary.
        #[allow(unsafe_code)]
        let ident = unsafe { from_utf8_unchecked(ident) };

        self.remaining = remaining;
        Some(Token::Ident(Ident(ident)))
    }

    fn next_ident(&mut self) -> Result<Ident<'a>, Error<'a>> {
        match self.next_token() {
            Some(Token::Ident(ident)) => Ok(ident),
            Some(other) => Err(Error::ExpectedIdent(other)),
            None => Err(Error::NoIdent),
        }
    }

    fn access_following(&mut self, token: Token<'a>) -> Result<Access<'a>, Error<'a>> {
        match token {
            Token::Dot => Ok(self.next_ident()?.field()),
            Token::Pound => self.next_ident()?.field_index(),
            Token::Ident(ident) => Ok(ident.field()),
            Token::CloseBracket => Err(Error::CloseBeforeOpen),
            Token::OpenBracket => {
                let index_ident = self.next_ident()?.list_index()?;
                match self.next_token() {
                    Some(Token::CloseBracket) => Ok(index_ident),
                    Some(other) => Err(Error::BadClose(other)),
                    None => Err(Error::Unclosed),
                }
            }
        }
    }

    fn offset(&self) -> usize {
        self.path.len() - self.remaining.len()
    }
}
impl<'a> Iterator for PathParser<'a> {
    type Item = (Result<Access<'a>, ReflectPathError<'a>>, usize);

    fn next(&mut self) -> Option<Self::Item> {
        let token = self.next_token()?;
        let offset = self.offset();
        Some((
            self.access_following(token)
                .map_err(|error| ReflectPathError::ParseError {
                    offset,
                    path: self.path,
                    error: ParseError(error),
                }),
            offset,
        ))
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Ident<'a>(&'a str);

impl<'a> Ident<'a> {
    fn field(self) -> Access<'a> {
        let field = |_| Access::Field(self.0.into());
        self.0.parse().map(Access::TupleIndex).unwrap_or_else(field)
    }
    fn field_index(self) -> Result<Access<'a>, Error<'a>> {
        Ok(Access::FieldIndex(self.0.parse()?))
    }
    fn list_index(self) -> Result<Access<'a>, Error<'a>> {
        Ok(Access::ListIndex(self.0.parse()?))
    }
}

// NOTE: We use repr(u8) so that the `match byte` in `Token::symbol_from_byte`
// becomes a "check `byte` is one of SYMBOLS and forward its value" this makes
// the optimizer happy, and shaves off a few cycles.
#[derive(Debug, PartialEq, Eq)]
#[repr(u8)]
enum Token<'a> {
    Dot = b'.',
    Pound = b'#',
    OpenBracket = b'[',
    CloseBracket = b']',
    Ident(Ident<'a>),
}
impl fmt::Display for Token<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Dot => f.write_char('.'),
            Token::Pound => f.write_char('#'),
            Token::OpenBracket => f.write_char('['),
            Token::CloseBracket => f.write_char(']'),
            Token::Ident(ident) => f.write_str(ident.0),
        }
    }
}
impl<'a> Token<'a> {
    const SYMBOLS: &'static [u8] = b".#[]";
    fn symbol_from_byte(byte: u8) -> Option<Self> {
        match byte {
            b'.' => Some(Self::Dot),
            b'#' => Some(Self::Pound),
            b'[' => Some(Self::OpenBracket),
            b']' => Some(Self::CloseBracket),
            _ => None,
        }
    }
}