// The error code of a write whose outcome is not known: the connection dropped after the request was sent, so it may or
// may not have been applied. The screen reconciles it by operation ID before anything is repeated (§4, §5).
export const OUTCOME_UNKNOWN = 'outcome_unknown';
