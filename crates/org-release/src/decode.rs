//! Private direct wire decoding: never route integers through JSON Value.

use crate::{Architecture, Platform, Role, MAX_FILES, MAX_TRUSTED_KEYS};
use serde::de::{self, IntoDeserializer, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::fmt;

/// Unlike an Option field, this wrapper has no missing-field default. The key
/// must occur exactly once, even when the value is explicitly null.
#[derive(Debug)]
pub(super) struct RequiredNullable<T>(pub Option<T>);

impl<'de, T: Deserialize<'de>> Deserialize<'de> for RequiredNullable<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // deserialize_any is intentional. Serde's synthetic missing-field
        // deserializer supplies None through deserialize_option, which would
        // make an Option-delegating wrapper silently accept an absent key.
        struct Nullable<T>(std::marker::PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for Nullable<T> {
            type Value = RequiredNullable<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a required null, string or unsigned integer")
            }

            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(RequiredNullable(None))
            }

            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
                T::deserialize(value.into_deserializer()).map(|value| RequiredNullable(Some(value)))
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                T::deserialize(value.into_deserializer()).map(|value| RequiredNullable(Some(value)))
            }
        }
        deserializer.deserialize_any(Nullable(std::marker::PhantomData))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Registry {
    pub version: u64,
    #[serde(deserialize_with = "registry_keys")]
    pub keys: Vec<Key>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Key {
    pub key_id: String,
    pub public_key_hex: String,
    pub min_sequence: u64,
    pub max_sequence: RequiredNullable<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Manifest {
    pub format: String,
    pub release_id: String,
    pub sequence: u64,
    pub platform: Platform,
    pub architecture: Architecture,
    pub installer_contract: u64,
    pub key_id: String,
    #[serde(deserialize_with = "manifest_files")]
    pub files: Vec<File>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct File {
    pub path: String,
    pub size: u64,
    pub sha256: String,
    pub role: Role,
    pub macos_cdhash: RequiredNullable<String>,
}

fn manifest_files<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<File>, D::Error> {
    bounded_sequence(deserializer, MAX_FILES)
}

fn registry_keys<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<Key>, D::Error> {
    bounded_sequence(deserializer, MAX_TRUSTED_KEYS)
}

fn bounded_sequence<'de, D, T>(deserializer: D, limit: usize) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Bounded<T>(usize, std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> Visitor<'de> for Bounded<T> {
        type Value = Vec<T>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a bounded array")
        }

        fn visit_seq<S: SeqAccess<'de>>(self, mut sequence: S) -> Result<Vec<T>, S::Error> {
            let mut values = Vec::new();
            while let Some(value) = sequence.next_element()? {
                if values.len() == self.0 {
                    return Err(de::Error::custom("array limit exceeded"));
                }
                values.push(value);
            }
            Ok(values)
        }
    }
    deserializer.deserialize_seq(Bounded(limit, std::marker::PhantomData))
}
