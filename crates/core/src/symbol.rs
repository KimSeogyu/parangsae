//! Exchange symbol newtype. Validation is ASCII uppercase and digits, 1 to 32 characters.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Symbol(String);

impl Symbol {
    pub fn new(raw: &str) -> Result<Self, SymbolError> {
        if raw.is_empty() || raw.len() > 32 {
            return Err(SymbolError::Length);
        }
        if !raw
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        {
            return Err(SymbolError::Charset);
        }
        Ok(Self(raw.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolError {
    Length,
    Charset,
}

impl fmt::Display for SymbolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Length => write!(f, "symbol length must be 1..=32"),
            Self::Charset => write!(f, "symbol must be ASCII uppercase or digits"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn btcusdt_is_accepted_and_lowercase_is_not() -> Result<(), String> {
        assert_eq!(
            Symbol::new("BTCUSDT")
                .map_err(|err| err.to_string())?
                .as_str(),
            "BTCUSDT"
        );
        assert_eq!(Symbol::new("btcusdt"), Err(SymbolError::Charset));
        Ok(())
    }
}
