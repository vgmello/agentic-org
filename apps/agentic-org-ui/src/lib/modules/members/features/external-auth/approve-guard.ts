// Pure submission-race guard for `ExternalAuthApproveDialog` (`TASK-249`).
// Aborts stale in-flight approvals on dialog close, target switch, or unmount, and gates every
// post-await UI update on matching `(abort, organizationId, requestId)` + live mount state.

export interface ApprovalSubmissionToken {
	readonly abort: AbortController;
	readonly targetOrg: string;
	readonly targetReqId: string | undefined;
}

export interface ApprovalSubmissionGuard {
	begin(targetOrg: string, targetReqId: string | undefined): ApprovalSubmissionToken;
	isCurrent(
		token: ApprovalSubmissionToken,
		currentOrg: string,
		currentReqId: string | undefined
	): boolean;
	finish(token: ApprovalSubmissionToken): void;
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
		begin(targetOrg: string, targetReqId: string | undefined): ApprovalSubmissionToken {
			cancel();
			const token: ApprovalSubmissionToken = {
				abort: new AbortController(),
				targetOrg,
				targetReqId
			};
			active = token;
			return token;
		},
		isCurrent(
			token: ApprovalSubmissionToken,
			currentOrg: string,
			currentReqId: string | undefined
		): boolean {
			return (
				active === token &&
				!destroyed &&
				!token.abort.signal.aborted &&
				currentOrg === token.targetOrg &&
				currentReqId === token.targetReqId
			);
		},
		finish(token: ApprovalSubmissionToken): void {
			if (active === token) active = undefined;
		},
		cancel,
		destroy(): void {
			destroyed = true;
			cancel();
		}
	};
}
