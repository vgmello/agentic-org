/**
 * The command that releases a new organization's recovery code on the HQ machine (ADR-0011).
 * The name is single-quoted for a POSIX shell, so no character of it is interpreted: a quote
 * inside the name closes the string, adds an escaped quote and opens it again.
 */
export const shellQuote = (value: string): string => `'${value.replaceAll("'", "'\\''")}'`;

export const recoveryReleaseCommand = (organizationName: string): string =>
	`agentic-org recovery release --org ${shellQuote(organizationName)}`;
