// Pure submission-race guard for `ExternalAuthApproveDialog` (`TASK-249`).
// Aborts stale in-flight approvals on dialog close, target switch, or unmount, and gates every
// post-await UI update on matching `(abort, organizationId, targetKey)` + live mount state.

export interface ApprovalSubmissionToken {
	readonly abort: AbortController;
	readonly targetOrg: string;
	readonly targetKey: string | undefined;
}

export interface ApprovalSubmissionGuard {
	begin(targetOrg: string, targetKey: string | undefined): ApprovalSubmissionToken;
	isCurrent(
		token: ApprovalSubmissionToken,
		currentOrg: string,
		currentKey: string | undefined
	): boolean;
	/** Clears `token` if still active; returns `true` iff this token owned the active slot. */
	finish(token: ApprovalSubmissionToken): boolean;
	cancel(): void;
	destroy(): void;
}

export function createApprovalSubmissionGuard(): ApprovalSubmissionGuard {
	let active: ApprovalSubmissionToken | undefined;
	let destroyed = false;

	function cancel(): void {
		active?.abort.abort();
		active = undefined;
	}

	return {
		begin(targetOrg: string, targetKey: string | undefined): ApprovalSubmissionToken {
			cancel();
			const token: ApprovalSubmissionToken = {
				abort: new AbortController(),
				targetOrg,
				targetKey
			};
			active = token;
			return token;
		},
		isCurrent(
			token: ApprovalSubmissionToken,
			currentOrg: string,
			currentKey: string | undefined
		): boolean {
			return (
				active === token &&
				!destroyed &&
				!token.abort.signal.aborted &&
				currentOrg === token.targetOrg &&
				currentKey === token.targetKey
			);
		},
		finish(token: ApprovalSubmissionToken): boolean {
			if (active !== token) return false;
			active = undefined;
			return true;
		},
		cancel,
		destroy(): void {
			destroyed = true;
			cancel();
		}
	};
}
