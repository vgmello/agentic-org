import { LINK_USED_COPY, UNREACHABLE_COPY } from './credential-copy';
import { SessionRequestError } from './session-gateway';

export type CliApprovalFailure =
	| { kind: 'link_used'; message: string }
	| { kind: 'auth_required'; message: string }
	| { kind: 'unreachable'; message: string }
	| { kind: 'failed'; message: string };

/** Maps any POST /api/v1/cli-approvals/decide failure to safe UI copy without server message text. */
export function describeCliApprovalFailure(error: unknown): CliApprovalFailure {
	if (error instanceof SessionRequestError) {
		if (error.status === 404) return { kind: 'link_used', message: LINK_USED_COPY };
		if (error.status === 401)
			return {
				kind: 'auth_required',
				message: 'Sign in to approve or decline this terminal sign-in.'
			};
		if (error.status === 503) return { kind: 'unreachable', message: UNREACHABLE_COPY };
		return { kind: 'failed', message: "HQ couldn't process this approval. Try again." };
	}
	return { kind: 'unreachable', message: UNREACHABLE_COPY };
}
