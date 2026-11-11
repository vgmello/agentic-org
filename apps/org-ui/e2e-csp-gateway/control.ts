export type Violation = { kind: 'violation'; documentURI: string; blockedURI: string; effectiveDirective: string };

/** Only the deliberately injected inline script on its marked document may be cleared. */
export function isControl(event: Violation, marker: string): boolean {
	return new URL(event.documentURI).searchParams.get('csp-control') === marker
		&& event.blockedURI === 'inline'
		&& ['script-src', 'script-src-elem'].includes(event.effectiveDirective);
}
