use crate::parse::{parse_decimal, parse_id};
use crate::Captures;
use std::io;

/// A set of options for expanding a template string using the contents
/// of capture groups.  Create using the `builder` method.
#[derive(Debug)]
pub struct Expander<'a> {
    sub_char: char,
    delimiters: Option<Delimiters<'a>>,
    allow_undelimited_name: bool,
    strict: bool,
}

#[derive(Debug)]
struct Delimiters<'a> {
    open: &'a str,
    close: &'a str,
}

impl Expander<'static> {
    /// Returns an expander that uses Python-compatible syntax.
    ///
    /// Expands all instances of `\num` or `\g<name>` in `replacement`
    /// to the corresponding capture group `num` or `name`, and writes
    /// them to the `dst` buffer given.
    ///
    /// `name` may be an integer corresponding to the index of the
    /// capture group (counted by order of opening parenthesis where `0` is the
    /// entire match) or it can be a name (consisting of letters, digits or
    /// underscores) corresponding to a named capture group.
    ///
    /// `num` must be an integer corresponding to the index of the
    /// capture group.
    ///
    /// If `num` or `name` isn't a valid capture group (whether the name doesn't exist
    /// or isn't a valid index), then it is replaced with the empty string.
    ///
    /// The longest possible number is used. e.g., `\10` looks up capture
    /// group 10 and not capture group 1 followed by a literal 0.
    ///
    /// To write a literal `\`, use `\\`.
    pub fn python() -> Self {
        Expander::builder('\\')
            .delimiters("g<", ">")
            .allow_undelimited_name(false)
            .build()
    }
}

impl<'a> Expander<'a> {
    /// Creates a new builder object used to initialize a new `Expander`.  The expander
    /// uses the character `sub_char` to introduce a substitution, with two consecutive
    /// occurrences of `sub_char` used to denote a literal `sub_char` in the expansion.
    ///
    /// By default, only numbered capture groups can be expanded by following the
    /// substitution character with zero or more decimal digits denoting the group
    /// number.
    pub fn builder(sub_char: char) -> ExpanderBuilder<'a> {
        ExpanderBuilder(Expander {
            sub_char,
            delimiters: None,
            allow_undelimited_name: false,
            strict: false,
        })
    }

    /// Expands the template string `template` using the syntax defined
    /// by this expander and the values of capture groups from `captures`.
    ///
    /// Always succeeds when this expander is not strict.
    pub fn expand<'t>(&self, captures: &Captures<'t>, template: &str) -> io::Result<String> {
        let mut cursor = io::Cursor::new(Vec::new());
        self.expand_to(&mut cursor, captures, template)?;
        Ok(String::from_utf8(cursor.into_inner()).expect("expansion is UTF-8"))
    }

    /// Expands the template string `template` using the syntax defined
    /// by this expander and the values of capture groups from `captures`.
    /// The output is appended to `dst`.
    ///
    /// Always succeeds when this expander is not strict.  When an error is
    /// reported, a partial expansion may be appended to `dst`.
    pub fn expand_to<'t>(
        &self,
        mut dst: impl io::Write,
        captures: &Captures<'t>,
        template: &str,
    ) -> io::Result<()> {
        let mut iter = template.char_indices();
        while let Some((index, c)) = iter.next() {
            if c == self.sub_char {
                let tail = iter.as_str();
                let skip = if tail.starts_with(self.sub_char) {
                    write!(dst, "{}", self.sub_char)?;
                    1
                } else if let Some((id, skip)) = self
                    .delimiters
                    .as_ref()
                    .and_then(|Delimiters { open, close }| {
                        debug_assert!(!open.is_empty());
                        debug_assert!(!close.is_empty());
                        parse_id(tail, open, close)
                    })
                    .or_else(|| {
                        if self.allow_undelimited_name {
                            parse_id(tail, "", "")
                        } else {
                            None
                        }
                    })
                {
                    if let Some(m) = captures.name(id) {
                        write!(dst, "{}", m.as_str())?;
                    } else if let Some(m) = id.parse().ok().and_then(|num| captures.get(num)) {
                        write!(dst, "{}", m.as_str())?;
                    } else if self.strict {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("invalid substitution group: {:?}", id),
                        ));
                    }
                    skip
                } else if let Some((skip, num)) = parse_decimal(tail, 0) {
                    if let Some(m) = captures.get(num) {
                        write!(dst, "{}", m.as_str())?;
                    }
                    skip
                } else if self.strict {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("invalid substitution sequence as position {}", index),
                    ));
                } else {
                    write!(dst, "{}", self.sub_char)?;
                    0
                };
                iter = iter.as_str()[skip..].char_indices();
            } else {
                write!(dst, "{}", c)?;
            }
        }
        Ok(())
    }
}

impl<'a> Default for Expander<'a> {
    fn default() -> Self {
        Expander::builder('$')
            .delimiters("{", "}")
            .allow_undelimited_name(true)
            .build()
    }
}

/// A builder object for constructing new `Expander` values.
#[derive(Debug)]
pub struct ExpanderBuilder<'a>(Expander<'a>);

impl<'a> ExpanderBuilder<'a> {
    /// Creates an expander using the current options in this builder.
    pub fn build(self) -> Expander<'a> {
        self.0
    }

    /// Sets an pair of non-empty delimiter strings used to enclose the name or number of
    /// a capture group in an expander's template string.  To be recognized, the group name or
    /// number surrounded by delimiters must immediately follow the substitution character
    /// set by `Expander::builder`.
    pub fn delimiters(mut self, open: &'a str, close: &'a str) -> Self {
        assert!(
            !open.is_empty() && !close.is_empty(),
            "Empty delimiter strings are not allowed."
        );
        self.0.delimiters = Some(Delimiters { open, close });
        self
    }

    /// By default, a capture group name must be enclosed by delimiters in order to
    /// be recognized.  Passing `true` to this method allows undelimited names to be
    /// recognized, where the name is taken to be the longest possible sequence of
    /// identifier characters following the substitution character.
    pub fn allow_undelimited_name(mut self, value: bool) -> Self {
        self.0.allow_undelimited_name = value;
        self
    }

    /// By default, `Expander::expand` always succeeds.  Invalid syntax in the template string
    /// is treated as literal text, and a substitution involving a group that failed to
    /// match, or a name that does not denote a capture group, is expanded to the empty string.
    /// Passing `true` to this method causes both situations to be reported as errors.
    pub fn strict(mut self, value: bool) -> Self {
        self.0.strict = value;
        self
    }
}
