//! `agentic-org agent bind`: one-step operator-authorized BYOA workspace binding (`TASK-211`).
//! All other `agent <verb>` commands (`inspect`, `update`, `pause`, `approve`, `sessions`,
//! `revoke`, ...) run over the shared office command table via `sign_in::office_proxy`.

mod bind;

pub(crate) use bind::{run, run_after_sign_in};
