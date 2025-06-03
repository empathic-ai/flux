use bevy::{prelude::*, reflect::{ReflectMut, ReflectRef}};
use core::fmt;

pub mod error;
pub use error::*;

mod parse;
use parse::*;
mod access;
use access::*;

use derive_more::derive::{Display, From};

type PathResult<'a, T> = Result<T, ReflectPathError<'a>>;

/// An error returned from a failed path string query.
#[derive(Debug, PartialEq, Eq, Display, From)]
pub enum ReflectPathError<'a> {
    /// An error caused by trying to access a path that's not able to be accessed,
    /// see [`AccessError`] for details.
    InvalidAccess(AccessError<'a>),

    /// An error that occurs when a type cannot downcast to a given type.
    #[display("Can't downcast result of access to the given type")]
    InvalidDowncast,

    /// An error caused by an invalid path string that couldn't be parsed.
    #[display("Encountered an error at offset {offset} while parsing `{path}`: {error}")]
    ParseError {
        /// Position in `path`.
        offset: usize,
        /// The path that the error occurred in.
        path: &'a str,
        /// The underlying error.
        error: ParseError<'a>,
    },
}

impl<'a> core::error::Error for ReflectPathError<'a> {}

#[derive(Clone, Debug, PartialEq, PartialOrd, Ord, Eq, Hash)]
pub struct OptionalParsedPath(
    /// This is a vector of pre-parsed [`OffsetAccess`]es.
    pub Vec<OffsetAccess>,
);

impl OptionalParsedPath {
    /// Parses a [`ParsedPath`] from a string.
    ///
    /// Returns an error if the string does not represent a valid path to an element.
    ///
    /// The exact format for path strings can be found in the documentation for [`GetPath`].
    /// In short, though, a path consists of one or more chained accessor strings.
    /// These are:
    /// - Named field access (`.field`)
    /// - Unnamed field access (`.1`)
    /// - Field index access (`#0`)
    /// - Sequence access (`[2]`)
    ///
    /// # Example
    /// ```
    /// # use bevy_reflect::{ParsedPath, Reflect, ReflectPath};
    /// #[derive(Reflect)]
    /// struct Foo {
    ///   bar: Bar,
    /// }
    ///
    /// #[derive(Reflect)]
    /// struct Bar {
    ///   baz: Baz,
    /// }
    ///
    /// #[derive(Reflect)]
    /// struct Baz(f32, Vec<Option<u32>>);
    ///
    /// let foo = Foo {
    ///   bar: Bar {
    ///     baz: Baz(3.14, vec![None, None, Some(123)])
    ///   },
    /// };
    ///
    /// let parsed_path = ParsedPath::parse("bar#0.1[2].0").unwrap();
    /// // Breakdown:
    /// //   "bar" - Access struct field named "bar"
    /// //   "#0" - Access struct field at index 0
    /// //   ".1" - Access tuple struct field at index 1
    /// //   "[2]" - Access list element at index 2
    /// //   ".0" - Access tuple variant field at index 0
    ///
    /// assert_eq!(parsed_path.element::<u32>(&foo).unwrap(), &123);
    /// ```
    pub fn parse(string: &str) -> PathResult<Self> {
        let mut parts = Vec::new();
        for (access, offset) in PathParser::new(string) {
            parts.push(OffsetAccess {
                access: access?.into_owned(),
                offset: Some(offset),
            });
        }
        Ok(Self(parts))
    }

    /// Similar to [`Self::parse`] but only works on `&'static str`
    /// and does not allocate per named field.
    pub fn parse_static(string: &'static str) -> PathResult<'static, Self> {
        let mut parts = Vec::new();
        for (access, offset) in PathParser::new(string) {
            parts.push(OffsetAccess {
                access: access?,
                offset: Some(offset),
            });
        }
        Ok(Self(parts))
    }
//}
//impl<'a> ReflectPath<'a> for &'a OptionalParsedPath {
    fn reflect_element<'a>(self, mut root: &dyn PartialReflect) -> PathResult<'a, &dyn PartialReflect> {
        for OffsetAccess { access, offset } in &self.0 {
            root = access.element(root, *offset)?;
        }
        Ok(root)
    }
    fn reflect_element_mut<'a>(
        self,
        mut root: &mut dyn PartialReflect,
    ) -> PathResult<'a, &mut dyn PartialReflect> {
        for OffsetAccess { access, offset } in &self.0 {
            root = access.element_mut(root, *offset)?;
        }
        Ok(root)
    }
}
impl<const N: usize> From<[OffsetAccess; N]> for OptionalParsedPath {
    fn from(value: [OffsetAccess; N]) -> Self {
        OptionalParsedPath(value.to_vec())
    }
}
impl From<Vec<Access<'static>>> for OptionalParsedPath {
    fn from(value: Vec<Access<'static>>) -> Self {
        OptionalParsedPath(
            value
                .into_iter()
                .map(|access| OffsetAccess {
                    access,
                    offset: None,
                })
                .collect(),
        )
    }
}
impl<const N: usize> From<[Access<'static>; N]> for OptionalParsedPath {
    fn from(value: [Access<'static>; N]) -> Self {
        value.to_vec().into()
    }
}

impl<'a> TryFrom<&'a str> for OptionalParsedPath {
    type Error = ReflectPathError<'a>;
    fn try_from(value: &'a str) -> Result<Self, Self::Error> {
        OptionalParsedPath::parse(value)
    }
}

impl fmt::Display for OptionalParsedPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for OffsetAccess { access, .. } in &self.0 {
            write!(f, "{access}")?;
        }
        Ok(())
    }
}
impl core::ops::Index<usize> for OptionalParsedPath {
    type Output = OffsetAccess;
    fn index(&self, index: usize) -> &Self::Output {
        &self.0[index]
    }
}
impl core::ops::IndexMut<usize> for OptionalParsedPath {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.0[index]
    }
}

/// An [`Access`] combined with an `offset` for more helpful error reporting.
#[derive(Clone, Debug, PartialEq, PartialOrd, Ord, Eq, Hash)]
pub struct OffsetAccess {
    /// The [`Access`] itself.
    pub access: Access<'static>,
    /// A character offset in the string the path was parsed from.
    pub offset: Option<usize>,
}

/// Extension trait for optionally unwrapping `Option` fields during reflection path traversal.
pub trait GetOptionalPath: GetPath {
    /// Attempts to retrieve a nested field via a pre-parsed path, automatically unwrapping `Option<T>`
    /// variants. Returns `None` if any access fails or an `Option` is `None`.
    fn reflect_optional_path<'p>(&self, path: &OptionalParsedPath) -> Option<&dyn PartialReflect> {
        let mut current: &dyn PartialReflect = self.as_partial_reflect();
        for offset_access in &path.0 {
            // Attempt to access the next element
            current = match offset_access.access.element(current, offset_access.offset) {
                Ok(value) => value,
                Err(_) => return None,
            };
            // If the current element is an enum, check for Option variants
            if let ReflectRef::Enum(enum_ref) = current.reflect_ref() {
                // `None` variant indicates missing value
                if enum_ref.variant_name() == "None" {
                    return None;
                }
                // Unwrap `Some(inner)` by extracting the single unnamed field
                if enum_ref.variant_name() == "Some" {
                    current = enum_ref.field_at(0)?;
                }
            }
        }
        Some(current)
    }

    /// Mutable version of [`reflect_optional_path`].
    fn reflect_optional_path_mut<'p>(&mut self, path: &OptionalParsedPath) -> Option<&mut dyn PartialReflect> {
        let mut current: &mut dyn PartialReflect = self.as_partial_reflect_mut();
        for offset_access in &path.0 {
            current = offset_access.access.element_mut(current, offset_access.offset).ok()?;
            current = handle_option_mut(current)?;
        }
        Some(current)
    }
}

/// Helper to detect and unwrap `Option` variants on a mutable `PartialReflect`.
fn handle_option_mut(current: &mut dyn PartialReflect) -> Option<&mut dyn PartialReflect> {
    // First pass: detect if current is Option<T> and which variant, without retaining borrow
    let variant = {
        if let ReflectMut::Enum(enum_ref) = current.reflect_mut() {
            if let Some(info) = enum_ref.get_represented_enum_info() {
                let path = info.type_path();
                if path.starts_with("std::option::Option") || path.starts_with("core::option::Option") {
                    Some(enum_ref.variant_name())
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    };
    // If this is Option::None, bail out
    if let Some("None") = variant {
        return None;
    }
    // If this is Option::Some, re-borrow and extract inner
    if let Some("Some") = variant {
        if let ReflectMut::Enum(enum_ref) = current.reflect_mut() {
            return enum_ref.field_at_mut(0);
        } else {
            return None;
        }
    }
    // Not an Option or non-None/Some variant: return current as-is
    Some(current)
}

// Blanket implementation for all types that implement `GetPath`
impl<T: GetPath + ?Sized> GetOptionalPath for T {}