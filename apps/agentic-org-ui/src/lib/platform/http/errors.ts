/** Only an adapter that guarantees no dispatch occurred may throw this error. Never wrap send failures in it. */
export class NotDispatchedError extends Error {
	readonly code = 'invalid_request';
	constructor(message = 'The request could not be submitted.') {
		super(message);
		this.name = 'NotDispatchedError';
	}
}
