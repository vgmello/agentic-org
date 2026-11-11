use std::{fmt, num::NonZeroU64, str::FromStr};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidValue;
impl fmt::Display for InvalidValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid replication value")
    }
}
impl std::error::Error for InvalidValue {}

macro_rules! identity {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);
        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, InvalidValue> {
                let value = value.into();
                if !(1..=64).contains(&value.len())
                    || !value.bytes().all(|b| b.is_ascii_alphanumeric())
                {
                    return Err(InvalidValue);
                }
                Ok(Self(value))
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl FromStr for $name {
            type Err = InvalidValue;
            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}
identity!(OrgId);
identity!(OfficeId);

macro_rules! sequence {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(NonZeroU64);
        impl $name {
            pub fn new(value: u64) -> Result<Self, InvalidValue> {
                NonZeroU64::new(value).map(Self).ok_or(InvalidValue)
            }
            pub fn get(self) -> u64 {
                self.0.get()
            }
            pub fn checked_next(self) -> Option<Self> {
                self.get().checked_add(1).and_then(|v| Self::new(v).ok())
            }
        }
    };
}
sequence!(Epoch);

/// Sequence zero represents a checkpoint before the first event in an epoch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Position {
    pub epoch: Epoch,
    pub seq: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Digest([u8; 32]);
impl Digest {
    pub const ZERO: Self = Self([0; 32]);
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Codec identity is distinct from the negotiated wire/schema version.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Codec {
    id: String,
    version: u32,
}
impl Codec {
    pub fn new(id: impl Into<String>, version: u32) -> Result<Self, InvalidValue> {
        let id = id.into();
        if version == 0
            || !(1..=64).contains(&id.len())
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return Err(InvalidValue);
        }
        Ok(Self { id, version })
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn version(&self) -> u32 {
        self.version
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identities_and_sequences_validate_without_imposing_ulid_only_ids() {
        for good in ["A", "01ORG123", &"a".repeat(64)] {
            OrgId::new(good).unwrap();
            OfficeId::new(good).unwrap();
        }
        for bad in ["", "../x", "é", "a/b", "a b", &"x".repeat(65)] {
            assert!(OrgId::new(bad).is_err());
            assert!(OfficeId::new(bad).is_err());
        }
        assert!(Epoch::new(0).is_err());
        assert!(Epoch::new(u64::MAX).unwrap().checked_next().is_none());
        assert_eq!(Epoch::new(1).unwrap().checked_next().unwrap().get(), 2);
    }
    #[test]
    fn codec_identity_is_bounded_and_versioned() {
        assert!(Codec::new("org.event-v1", 1).is_ok());
        for (id, version) in [("", 1), ("org/event", 1), ("org", 0)] {
            assert!(Codec::new(id, version).is_err());
        }
    }
}
