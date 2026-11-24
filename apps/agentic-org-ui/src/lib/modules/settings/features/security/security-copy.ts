import { NotDispatchedError } from '$lib/platform/http/errors';
import type { HstsMode, ListenScheme } from '$lib/platform/http/wire';
import { SessionRequestError } from '$lib/platform/auth/session-gateway';
import { UNREACHABLE_COPY } from '$lib/platform/auth/credential-copy';

export type TlsMode = 'self_signed' | 'supplied';

export function listenSchemeLabel(scheme: ListenScheme): string {
	switch (scheme) {
		case 'http_loopback':
			return 'HTTP (loopback)';
		case 'http_published':
			return 'HTTP (published)';
		case 'https':
			return 'HTTPS';
		default:
			return 'Unknown';
	}
}

export function hstsLabel(hsts: HstsMode): string {
	switch (hsts) {
		case 'off':
			return 'Off (do not send Strict-Transport-Security)';
		case 'on':
			return 'On (enable Strict-Transport-Security)';
		case 'clear':
			return 'Clear (max-age=0)';
	}
}

export function isAuthAlreadyEnabled(error: unknown): boolean {
	return error instanceof SessionRequestError && error.status === 409;
}

export function validateTlsFields(
	mode: TlsMode,
	cert: string,
	key: string
): { cert?: string; key?: string } | undefined {
	if (mode !== 'supplied') return undefined;
	const errors: { cert?: string; key?: string } = {};
	if (!cert.trim()) errors.cert = 'Paste the PEM certificate chain.';
	if (!key.trim()) errors.key = 'Paste the PEM private key.';
	return errors.cert || errors.key ? errors : undefined;
}

export function describeTlsFailure(error: unknown): string {
	if (error instanceof SessionRequestError) {
		if (error.status === 409) return 'Enable authentication above before turning on HTTPS.';
		if (error.status === 403) return 'Only the instance administrator can configure HTTPS.';
		if (error.status === 401) return 'Your session ended. Sign in again to configure HTTPS.';
		if (error.status === 400)
			return 'The certificate or private key is invalid. Check that both are valid PEM blocks matching this host.';
		if (error.status === 503) return UNREACHABLE_COPY;
		return "HQ couldn't update HTTPS settings. Try again.";
	}
	if (error instanceof NotDispatchedError)
		return 'Enter a valid TLS configuration before submitting.';
	return UNREACHABLE_COPY;
}
