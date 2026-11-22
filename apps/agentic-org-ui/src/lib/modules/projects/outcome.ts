// The error code of a write whose outcome is not known: the connection dropped after the request was sent, so it may
// or may not have been applied (work-items-ux §12: lost responses resolve through the same operation identity, never a
// duplicate create). The screen asks `createOperation` by the same operation ID before anything is sent again. The same
// wire code as the costs module's (`OUTCOME_UNKNOWN` there); each module names the contract it owns.
export const OUTCOME_UNKNOWN = 'outcome_unknown';
