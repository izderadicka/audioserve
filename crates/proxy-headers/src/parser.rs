#![allow(clippy::type_complexity)]
use std::{borrow::Cow, str::Utf8Error, string::FromUtf8Error};

use nom::{
    branch::alt,
    bytes::complete::{escaped_transform, tag, take_while1, take_while_m_n},
    character::complete::space0,
    combinator::{cut, map, rest},
    multi::separated_list1,
    sequence::{preceded, separated_pair, terminated},
    AsChar, IResult, Parser,
};

pub type Error<'a> = nom::error::Error<&'a [u8]>;

const QUOTE: &[u8] = b"\"";
const TOKEN_CHARS: &[u8] = b"!#$%&'*+-.^_`|~:"; // we also add : to token - for XForwardedFor compatibility
const OBS_CHARS: &[u8] = b"._-";
const SCHEME_CHARS: &[u8] = b"+-.";
const HOST_CHARS: &[u8] = b"-.:"; // TODO I had problem to track down what exactly is allowed for host in RFC7230, so let's keep it now conservative

macro_rules! def_set {
    ($($name:ident = $chars: expr),*) => {
        $(
            pub fn $name(input: &[u8]) -> IResult<&[u8], &[u8]> {
                let is_char = |c: u8| c.is_alphanum() || $chars.contains(&c);
                take_while1(is_char).parse(input)
            }

        )*

    };
}

def_set!(
    token = TOKEN_CHARS,
    obs = OBS_CHARS,
    scheme = SCHEME_CHARS,
    host = HOST_CHARS
);

#[derive(Debug)]
pub struct StringError;

impl<E> From<nom::Err<E>> for StringError {
    fn from(_: nom::Err<E>) -> Self {
        StringError
    }
}

impl From<FromUtf8Error> for StringError {
    fn from(_: FromUtf8Error) -> Self {
        StringError
    }
}

impl From<Utf8Error> for StringError {
    fn from(_: Utf8Error) -> Self {
        StringError
    }
}

pub fn full_string<T, P>(i: &T, mut p: P) -> Result<String, StringError>
where
    T: AsRef<[u8]>,

    P: FnMut(&[u8]) -> IResult<&[u8], &[u8]>,
{
    let (left, res) = p(i.as_ref())?;
    if !left.is_empty() {
        return Err(StringError);
    }
    String::from_utf8(res.into()).map_err(Into::into)
}

pub fn all_string<T>(i: &T) -> Result<&str, StringError>
where
    T: AsRef<[u8]>,
{
    let (_left, res) = rest::<_, nom::error::Error<_>>(i.as_ref())?;
    std::str::from_utf8(res).map_err(Into::into)
}

pub fn quoted_string(input: &[u8]) -> IResult<&[u8], Vec<u8>> {
    let escaped = escaped_transform(
        take_while1(is_quoted_text),
        '\\',
        take_while_m_n(1, 1, is_escapable),
    );
    preceded(tag(QUOTE), cut(terminated(escaped, tag(QUOTE)))).parse(input)
}

fn is_quoted_text(c: u8) -> bool {
    // RFC 7230 qdtext         = HTAB / SP /%x21 / %x23-5B / %x5D-7E / obs-text
    // obs-text       = %x80-FF
    // but / and "  can be only escaped
    c != b'\\'
        && c != b'"'
        && (c == b'\t'
            || c == b' '
            || (0x23..=0x5b).contains(&c)
            || (0x4d..=0x7e).contains(&c)
            || c >= 0x80)
}

fn is_escapable(c: u8) -> bool {
    // RFC 7230   quoted-pair    = "\" ( HTAB / SP / VCHAR / obs-text )
    // RFC 5234  VCHAR          =  %x21-7E
    c == b'\t' || (0x20..=0x7e).contains(&c) || c >= 0x80
}

pub fn value(input: &[u8]) -> IResult<&[u8], Cow<'_, [u8]>> {
    alt((map(token, Cow::Borrowed), map(quoted_string, Cow::Owned))).parse(input)
}

pub fn values_list(input: &[u8]) -> IResult<&[u8], Vec<Cow<'_, [u8]>>> {
    terminated(separated_list1((tag(","), space0), value), space0).parse(input)
}

pub fn pair(input: &[u8]) -> IResult<&[u8], (&[u8], Cow<'_, [u8]>)> {
    separated_pair(token, tag("="), value).parse(input)
}

pub fn element(input: &[u8]) -> IResult<&[u8], Vec<(&[u8], Cow<'_, [u8]>)>> {
    separated_list1(tag(";"), pair).parse(input)
}

pub fn elements(input: &[u8]) -> IResult<&[u8], Vec<Vec<(&[u8], Cow<'_, [u8]>)>>> {
    separated_list1((tag(","), space0), element).parse(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escaped() {
        fn esc(i: &[u8]) -> IResult<&[u8], Vec<u8>> {
            escaped_transform(
                take_while1(|c| c != b'"' && c != b'\\'),
                '\\',
                take_while_m_n(1, 1, is_escapable),
            )(i)
        }
        let x: &[u8] = b"abcde";
        let (left, res) = esc(x).unwrap();
        assert!(left.is_empty());
        assert_eq!(res, x);

        let x: &[u8] = br#"abcde\"ef""#;
        let (left, res) = esc(x).unwrap();
        assert_eq!(left, b"\"");
        assert_eq!(res, br#"abcde"ef"#);

        let x: &[u8] = b"";
        let (left, res) = esc(x).unwrap();
        assert!(left.is_empty());
        assert!(res.is_empty());
    }

    #[test]
    fn test_quoted_chars() {
        assert!(is_quoted_text(b'a'));
        assert!(!is_quoted_text(b'\\'));
        assert!(!is_quoted_text(b'"'));
    }

    #[test]
    fn test_multiple_elements() {
        let m = "a=b,c=d,  e=f;g=h";
        let (left, es) = elements(m.as_bytes()).unwrap();
        assert!(left.is_empty());
        assert_eq!(es.len(), 3);
        assert_eq!(es[2].len(), 2);
    }

    #[test]
    fn basic_test_element() {
        let elem = b"hey=how;lets=\"[go](home)\"";
        let (left, e) = element(elem).unwrap();
        assert!(left.is_empty());
        assert_eq!(e.len(), 2);
        assert_eq!(
            e,
            vec![
                (&b"hey"[..], Cow::Borrowed(&b"how"[..])),
                (&b"lets"[..], Cow::Borrowed(&b"[go](home)"[..]))
            ]
        );
    }

    #[test]
    fn basic_quoted_string() {
        let corr = br#""usak kulisak""#;
        let (left, res) = quoted_string(corr).unwrap();
        assert!(left.is_empty());
        assert_eq!(res, b"usak kulisak");

        let incorr = br#""usak kulisak"#;
        let e = quoted_string(incorr);
        assert!(e.is_err());
    }

    #[test]
    fn escaped_quoted_string() {
        let s: &[u8] = br#""this is \"escaped\" and \\""#;
        let (left, res) = quoted_string(s).unwrap();
        assert!(left.is_empty());
        assert_eq!(res, br#"this is "escaped" and \"#);
    }

    #[test]
    fn basic_token_test() {
        let ok = b"127.0.0.1";
        let (left, t) = token(ok).unwrap();
        assert_eq!(t, ok);
        assert!(left.is_empty());

        let quoted = br#""usak kulisak""#;
        let r = token(quoted);
        assert!(r.is_err());

        let spaced = b"aaa bbb";
        let (left, t) = token(spaced).unwrap();
        assert_eq!(left, b" bbb");
        assert_eq!(t, b"aaa");
    }
}
