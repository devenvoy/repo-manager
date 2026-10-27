//! Domain errors and invariant validation failures.

use std::{error::Error, fmt};

/// A validation failure at a domain construction boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValidationError {
    Empty(&'static str),
    TooLong {
        field: &'static str,
        max: usize,
    },
    InvalidCharacter(&'static str),
    InvalidPath(&'static str),
    InvalidRange {
        field: &'static str,
        min: u64,
        max: u64,
    },
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty(field) => write!(f, "{field} cannot be empty"),
            Self::TooLong { field, max } => write!(f, "{field} cannot exceed {max} characters"),
            Self::InvalidCharacter(field) => write!(f, "{field} contains an invalid character"),
            Self::InvalidPath(msg) => write!(f, "invalid path: {msg}"),
            Self::InvalidRange { field, min, max } => {
                write!(f, "{field} must be between {min} and {max}")
            }
        }
    }
}

impl Error for ValidationError {}

/// A typed domain or boundary error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DomainError {
    Validation(ValidationError),
    NotFound { entity: &'static str, id: String },
    Conflict { message: String },
    Unauthorized { message: String },
    PortFailure { port: &'static str, message: String },
}

impl fmt::Display for DomainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(err) => write!(f, "validation failed: {err}"),
            Self::NotFound { entity, id } => write!(f, "{entity} not found: {id}"),
            Self::Conflict { message } => write!(f, "conflict: {message}"),
            Self::Unauthorized { message } => write!(f, "unauthorized: {message}"),
            Self::PortFailure { port, message } => write!(f, "{port} error: {message}"),
        }
    }
}

impl Error for DomainError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Validation(err) => Some(err),
            _ => None,
        }
    }
}

impl From<ValidationError> for DomainError {
    fn from(err: ValidationError) -> Self {
        Self::Validation(err)
    }
}

/// Helper for validated name construction boundaries.
pub(crate) fn validate_name(
    field: &'static str,
    raw: &str,
    max: usize,
    allowed: impl Fn(char) -> bool,
) -> Result<String, ValidationError> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(ValidationError::Empty(field));
    }
    if name.chars().count() > max {
        return Err(ValidationError::TooLong { field, max });
    }
    if !name.chars().all(allowed) {
        return Err(ValidationError::InvalidCharacter(field));
    }
    Ok(name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_error_display() {
        assert_eq!(
            ValidationError::Empty("name").to_string(),
            "name cannot be empty"
        );
        assert_eq!(
            ValidationError::TooLong {
                field: "name",
                max: 10
            }
            .to_string(),
            "name cannot exceed 10 characters"
        );
        assert_eq!(
            ValidationError::InvalidPath("no escape").to_string(),
            "invalid path: no escape"
        );
    }

    #[test]
    fn domain_error_display() {
        let not_found = DomainError::NotFound {
            entity: "Project",
            id: "p1".into(),
        };
        assert_eq!(not_found.to_string(), "Project not found: p1");
    }
}
