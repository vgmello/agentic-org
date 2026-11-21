import type { IncomingMessage } from 'node:http';

export type FakeLinkEntry =
	| { kind: 'administrator_registration' }
	| {
			kind: 'organization_invite';
			org_id: string;
			org_name: string;
			member_id: string;
			member_name?: string;
	  }
	| { kind: 'set_password'; login: string };

export interface AccountLinkState {
	links: Map<string, FakeLinkEntry>;
	currentPassword: string;
	passwordChanges: { current_password: string; new_password: string }[];
	registered: { mode: 'create' | 'attach'; login: string; link: string }[];
	passwordResets: { login: string; link: string }[];
}

export function createAccountLinkState(): AccountLinkState {
	return {
		links: new Map(),
		currentPassword: 'private-password',
		passwordChanges: [],
		registered: [],
		passwordResets: []
	};
}

export const ACCOUNT_PUBLIC_PATHS = new Set([
	'/api/v1/links/inspect',
	'/api/v1/links/register',
	'/api/v1/links/register/sign-in',
	'/api/v1/links/set-password'
]);

export const ACCOUNT_PROTECTED_PATHS = new Set(['/api/v1/account/password']);

type JsonReply = (value: unknown, status?: number) => void;
type EstablishFn = (login: string, admin: boolean) => void;

export async function handlePublicAccountRoute(
	pathname: string,
	req: IncomingMessage,
	readBody: (req: IncomingMessage) => Promise<Record<string, unknown>>,
	state: AccountLinkState,
	json: JsonReply,
	establish: EstablishFn
): Promise<boolean> {
	if (!ACCOUNT_PUBLIC_PATHS.has(pathname) || req.method !== 'POST') return false;
	const sent = await readBody(req);
	const link = typeof sent?.link === 'string' ? sent.link : '';
	const entry = state.links.get(link);

	if (pathname === '/api/v1/links/inspect') {
		if (!entry) {
			json({ error: { code: 'not_found', message: 'Link expired or used.' } }, 404);
			return true;
		}
		const kind = sent?.kind;
		const valid =
			(kind === 'set_password' && entry.kind === 'set_password') ||
			(kind === 'registration' && entry.kind !== 'set_password');
		if (!valid) {
			json({ error: { code: 'not_found', message: 'Link expired or used.' } }, 404);
			return true;
		}
		json({ result: entry }, 200);
		return true;
	}

	if (pathname === '/api/v1/links/set-password') {
		const password = typeof sent?.password === 'string' ? sent.password : '';
		if (!entry || entry.kind !== 'set_password') {
			json({ error: { code: 'not_found', message: 'Link expired or used.' } }, 404);
			return true;
		}
		if (password.length < 12) {
			json(
				{
					error: {
						code: 'invalid_request',
						field: 'password',
						rule: 'too_short',
						message: 'Invalid password.'
					}
				},
				400
			);
			return true;
		}
		state.links.delete(link);
		state.currentPassword = password;
		state.passwordResets.push({ login: entry.login, link });
		json({ result: { login: entry.login } }, 200);
		return true;
	}

	return handleRegisterRoute(pathname, sent, link, entry, state, json, establish);
}

function handleRegisterRoute(
	pathname: string,
	sent: Record<string, unknown>,
	link: string,
	entry: FakeLinkEntry | undefined,
	state: AccountLinkState,
	json: JsonReply,
	establish: EstablishFn
): boolean {
	const login = typeof sent?.login === 'string' ? sent.login.trim() : '';
	const password = typeof sent?.password === 'string' ? sent.password : '';
	if (!entry || entry.kind === 'set_password') {
		json({ error: { code: 'not_found', message: 'Link expired or used.' } }, 404);
		return true;
	}
	if (!login || !password) {
		json({ error: { code: 'invalid_request', message: 'Invalid credentials.' } }, 400);
		return true;
	}
	if (pathname === '/api/v1/links/register/sign-in') {
		if (password !== state.currentPassword) {
			json({ error: { code: 'not_authorized', message: 'Mismatch.' } }, 401);
			return true;
		}
		state.links.delete(link);
		state.registered.push({ mode: 'attach', login, link });
		establish(login, login === 'admin');
		return true;
	}
	if (login.toLowerCase() === 'taken') {
		json(
			{
				error: {
					code: 'invalid_request',
					field: 'login',
					rule: 'taken',
					message: 'Username taken.'
				}
			},
			400
		);
		return true;
	}
	state.links.delete(link);
	if (entry.kind === 'administrator_registration') {
		state.currentPassword = password;
	}
	state.registered.push({ mode: 'create', login, link });
	establish(login, entry.kind === 'administrator_registration');
	return true;
}

export async function handleChangePasswordRoute(
	pathname: string,
	req: IncomingMessage,
	readBody: (req: IncomingMessage) => Promise<Record<string, unknown>>,
	state: AccountLinkState,
	json: JsonReply
): Promise<boolean> {
	if (pathname !== '/api/v1/account/password' || req.method !== 'POST') return false;
	const sent = await readBody(req);
	const current = typeof sent?.current_password === 'string' ? sent.current_password : '';
	const next = typeof sent?.new_password === 'string' ? sent.new_password : '';
	if (!current || !next) {
		json({ error: { code: 'invalid_request', message: 'Invalid password change request.' } }, 400);
		return true;
	}
	if (current !== state.currentPassword) {
		json({ error: { code: 'not_authorized', message: 'Current password is incorrect.' } }, 403);
		return true;
	}
	if (next.length < 12) {
		json(
			{
				error: {
					code: 'invalid_request',
					field: 'password',
					rule: 'too_short',
					message: 'Password too short.'
				}
			},
			400
		);
		return true;
	}
	state.currentPassword = next;
	state.passwordChanges.push({ current_password: current, new_password: next });
	json({ result: {} }, 200);
	return true;
}
