//! The contract every typed call of the API implements ([`Call`]) and the empty params or
//! result ([`Empty`]). A leaf: the host calls (`calls`), the organization calls (`org`) and
//! the work calls (`work`) all build on it, and none of them on each other through it.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// One method of the API: its wire name and the shapes of its params and result.
/// Params and results are JSON objects (structs), so fields can be added later.
pub trait Call {
    const METHOD: &'static str;
    type Params: Serialize + DeserializeOwned;
    type Result: Serialize + DeserializeOwned;
}

/// Params or result with no fields. Serializes as `{}`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Empty {}
