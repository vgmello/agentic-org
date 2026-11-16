import {
	expect,
	type ConsoleMessage,
	type Locator,
	type Page,
	type Request,
	type TestInfo
} from '@playwright/test';
import { createHash, randomUUID } from 'node:crypto';
import {
	closeSync,
	constants,
	fstatSync,
	fsyncSync,
	lstatSync,
	linkSync,
	openSync,
	readFileSync,
	realpathSync,
	writeFileSync,
	unlinkSync
} from 'node:fs';
import { dirname, join, resolve } from 'node:path';

type BrowserTask = { taskId: string; title: string; initialRevision: string };
export type Credentials = { login: string; password: string };
type RevocationControl = { requestPath: string; receiptPath: string };
export type SessionIdentity = { accountId: string; instanceId: string; proof: string };
export type Fixture = BrowserTask & {
	origin: string;
	login: string;
	password: string;
	organizationId: string;
	officeId: string;
	accountableTeamId: string;
	assignedTeamId: string;
	employeeId: string;
	largeBodyTail: string;
	otherTaskId: string;
	foreignAccount: Credentials;
	accountRevocation: RevocationControl;
};
export type Pair = { assigned_team_id: string | null; assignee_id: string | null };
export type Intent = {
	operation_id: string;
	scope: { kind: string; instance_id: string; org_id: string; office_id: string };
	params: Pair & { expected_revision: string };
};
export type PublicReference = {
	task_id: string;
	revision: string;
	kind: 'snapshot' | 'event';
	sha256: string;
	byte_length: number;
};
export type Reply = { status: number; body: Record<string, unknown> };
const secrets = new Set<string>();
export function rememberSecret(value: string) {
	if (value) secrets.add(value);
}
export function sanitizeErrors(info: TestInfo) {
	for (const error of info.errors) {
		for (const key of ['message', 'stack', 'errorContext'] as const) {
			let value = error[key];
			if (!value) continue;
			for (const secret of secrets) {
				for (const form of [
					secret,
					JSON.stringify(secret).slice(1, -1),
					encodeURIComponent(secret)
				])
					value = value.replaceAll(form, '[redacted generated credential]');
			}
			error[key] = value;
		}
	}
}
const object = (value: unknown): Record<string, unknown> => {
	if (!value || typeof value !== 'object' || Array.isArray(value))
		throw new Error('Invalid public fixture or service shape');
	return value as Record<string, unknown>;
};
function required(value: unknown): string {
	if (typeof value !== 'string' || !value) throw new Error('Incomplete actual service fixture');
	return value;
}
function requireAbsentControl(path: string, descriptorPath: string) {
	if (
		resolve(path) !== path ||
		dirname(path) !== dirname(descriptorPath) ||
		path === descriptorPath ||
		realpathSync(dirname(path)) !== dirname(path)
	)
		throw new Error('Unsafe fixture account-revocation control path');
	let accessible = true;
	try {
		lstatSync(path);
	} catch (error) {
		if ((error as NodeJS.ErrnoException).code === 'ENOENT') return;
		accessible = false;
	}
	// Raw filesystem causes can include the private descriptor/control path.
	throw new Error(
		accessible
			? 'Fixture account-revocation control must initially be absent'
			: 'Cannot verify fixture account-revocation control path'
	);
}
type CommonFixture = Omit<
	Fixture,
	keyof BrowserTask | 'otherTaskId' | 'foreignAccount' | 'accountRevocation'
>;
function readFixtureDescriptor(path: string): Record<string, unknown> {
	let fd: number | undefined;
	try {
		const parent = lstatSync(dirname(path));
		if (
			!resolve(path).startsWith('/private/tmp/') ||
			realpathSync(path) !== resolve(path) ||
			parent.isSymbolicLink() ||
			!parent.isDirectory() ||
			parent.uid !== process.getuid?.() ||
			(parent.mode & 0o777) !== 0o700
		)
			throw new Error();
		fd = openSync(path, constants.O_RDONLY | constants.O_NOFOLLOW);
		const m = fstatSync(fd);
		if (
			!m.isFile() ||
			m.nlink !== 1 ||
			m.uid !== process.getuid?.() ||
			(m.mode & 0o777) !== 0o600 ||
			m.size > 16_384
		)
			throw new Error();
		return object(JSON.parse(readFileSync(fd, 'utf8')));
	} catch {
		// JSON/parser/filesystem exceptions may echo the private descriptor.
		throw new Error('Unsafe or invalid selected fixture descriptor');
	} finally {
		if (fd !== undefined) closeSync(fd);
	}
}
function commonFixture(raw: Record<string, unknown>): CommonFixture {
	const common = Object.fromEntries(
		[
			'origin',
			'login',
			'password',
			'organizationId',
			'officeId',
			'accountableTeamId',
			'assignedTeamId',
			'employeeId',
			'largeBodyTail'
		].map((key) => [key, required(raw[key])])
	) as CommonFixture;
	rememberSecret(common.login);
	rememberSecret(common.password);
	return common;
}
function fixtureAuth(raw: Record<string, unknown>, login: string, path: string) {
	const foreign = object(raw.foreignAccount);
	const control = object(raw.accountRevocation);
	if (
		Object.keys(foreign).sort().join(',') !== 'login,password' ||
		Object.keys(control).sort().join(',') !== 'receiptPath,requestPath'
	)
		throw new Error('Fixture auth extension must use the agreed private interface');
	const foreignAccount = { login: required(foreign.login), password: required(foreign.password) };
	rememberSecret(foreignAccount.login);
	rememberSecret(foreignAccount.password);
	if (foreignAccount.login === login)
		throw new Error('Foreign fixture account must have a distinct login');
	const accountRevocation = {
		requestPath: required(control.requestPath),
		receiptPath: required(control.receiptPath)
	};
	if (accountRevocation.requestPath === accountRevocation.receiptPath)
		throw new Error('Fixture account-revocation control paths must be distinct');
	requireAbsentControl(accountRevocation.requestPath, resolve(path));
	requireAbsentControl(accountRevocation.receiptPath, resolve(path));
	return { foreignAccount, accountRevocation };
}
function browserTask(value: unknown): BrowserTask {
	const task = object(value);
	const item = {
		taskId: required(task.taskId),
		title: required(task.title),
		initialRevision: required(task.initialRevision)
	};
	if (
		!/^[1-9][0-9]{0,18}$/.test(item.initialRevision) ||
		BigInt(item.initialRevision) <= 9007199254740991n ||
		BigInt(item.initialRevision) > 9223372036854775807n - 64n
	)
		throw new Error('Fixture requires an exact large i64 revision and 64 revisions of headroom');
	return item;
}
function fixtureTasks(raw: Record<string, unknown>) {
	const browsers = object(raw.browsers);
	return ['chromium', 'webkit'].map((name) => [name, browserTask(browsers[name])] as const);
}
function requireIndependentFixture(common: CommonFixture, tasks: ReturnType<typeof fixtureTasks>) {
	if (
		!/^https?:\/\/(localhost|127\.0\.0\.1):[0-9]+$/.test(common.origin) ||
		common.accountableTeamId === common.assignedTeamId ||
		tasks[0][1].taskId === tasks[1][1].taskId ||
		tasks[0][1].title === tasks[1][1].title
	)
		throw new Error('Fixture must use loopback and independent teams and browser tasks');
}
export function fixture(browser: string): Fixture {
	const path = process.env.SELECTED_TASK_FIXTURE;
	if (!path)
		throw new Error('SELECTED_TASK_FIXTURE must name an actual disposable Rust service fixture');
	const raw = readFixtureDescriptor(path);
	const common = commonFixture(raw);
	const { foreignAccount, accountRevocation } = fixtureAuth(raw, common.login, path);
	const tasks = fixtureTasks(raw);
	requireIndependentFixture(common, tasks);
	const selected = tasks.find(([name]) => name === browser)?.[1];
	const other = tasks.find(([name]) => name !== browser)?.[1];
	if (!selected || !other) throw new Error('Unsupported selected browser fixture');
	return { ...common, ...selected, otherTaskId: other.taskId, foreignAccount, accountRevocation };
}
function privateControlDirectory(control: RevocationControl) {
	const directory = dirname(control.requestPath);
	if (
		resolve(control.requestPath) !== control.requestPath ||
		resolve(control.receiptPath) !== control.receiptPath ||
		dirname(control.receiptPath) !== directory ||
		control.requestPath === control.receiptPath ||
		!directory.startsWith('/private/tmp/') ||
		realpathSync(directory) !== directory
	)
		throw new Error('Unsafe fixture revocation directory');
	const stat = lstatSync(directory);
	if (
		!stat.isDirectory() ||
		stat.isSymbolicLink() ||
		stat.uid !== process.getuid?.() ||
		(stat.mode & 0o777) !== 0o700
	)
		throw new Error('Unsafe fixture revocation directory');
	return directory;
}
function privateControlFile(fd: number, maximum: number, publishing = false) {
	const stat = fstatSync(fd);
	if (
		!stat.isFile() ||
		stat.uid !== process.getuid?.() ||
		(stat.mode & 0o777) !== 0o600 ||
		(stat.nlink !== 1 && !(publishing && stat.nlink === 2)) ||
		stat.size > maximum
	)
		throw new Error('Unsafe fixture revocation file');
	return stat;
}
export function publishRevocationRequest(control: RevocationControl) {
	let stage: string | undefined;
	let stageCreated = false;
	let fd: number | undefined;
	try {
		const directory = privateControlDirectory(control);
		requireAbsentControl(control.requestPath, control.receiptPath);
		requireAbsentControl(control.receiptPath, control.requestPath);
		const requestId = randomUUID();
		stage = join(directory, `.account-revocation-${requestId}.stage`);
		fd = openSync(
			stage,
			constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | constants.O_NOFOLLOW,
			0o600
		);
		stageCreated = true;
		privateControlFile(fd, 4096);
		writeFileSync(fd, JSON.stringify({ version: 1, requestId }) + '\n', 'utf8');
		fsyncSync(fd);
		closeSync(fd);
		fd = undefined;
		// The final name appears only after the complete stage is closed. link is
		// exclusive, so a competing existing target is never overwritten.
		linkSync(stage, control.requestPath);
		unlinkSync(stage);
		stageCreated = false;
		fd = openSync(directory, constants.O_RDONLY | constants.O_DIRECTORY | constants.O_NOFOLLOW);
		fsyncSync(fd);
		closeSync(fd);
		fd = undefined;
		return requestId;
	} catch {
		// Never retry a potentially published account-wide operation automatically.
		throw new Error('Cannot publish the private fixture revocation request');
	} finally {
		if (fd !== undefined) closeSync(fd);
		if (stageCreated && stage) unlinkSync(stage);
	}
}
export async function revokeOwnerAccount(control: RevocationControl, accountId: string) {
	const requestId = publishRevocationRequest(control);
	await expect
		.poll(
			() => {
				let fd: number | undefined;
				try {
					privateControlDirectory(control);
					try {
						fd = openSync(control.receiptPath, constants.O_RDONLY | constants.O_NOFOLLOW);
					} catch (error) {
						if ((error as NodeJS.ErrnoException).code === 'ENOENT') return false;
						throw error;
					}
					const before = privateControlFile(fd, 4096, true);
					// A hard-link publication can briefly expose nlink=2. Consume only the
					// complete singly linked receipt; all other malformed states fail.
					if (before.nlink === 2) return false;
					if (before.size === 0) throw new Error();
					const receipt = object(JSON.parse(readFileSync(fd, 'utf8')));
					const after = privateControlFile(fd, 4096);
					if (
						before.size !== after.size ||
						before.mtimeMs !== after.mtimeMs ||
						Object.keys(receipt).sort().join(',') !== 'accountId,requestId,status,version' ||
						receipt.version !== 1 ||
						receipt.requestId !== requestId ||
						receipt.accountId !== accountId ||
						receipt.status !== 'completed'
					)
						throw new Error();
					return true;
				} catch {
					throw new Error('Unsafe, stale, failed or mismatched fixture revocation receipt');
				} finally {
					if (fd !== undefined) closeSync(fd);
				}
			},
			{ message: 'Actual protected account-wide revocation must publish its correlated completion' }
		)
		.toBe(true);
}
export const asObject = object;
export const taskBase = (f: Fixture) =>
	`/api/v1/organizations/${encodeURIComponent(f.organizationId)}/tasks/${encodeURIComponent(f.taskId)}`;
export const assignmentPath = (f: Fixture) => `${taskBase(f)}/assignment`;
export const escapeRegex = (value: string) => value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
export const taskButton = (page: Page, f: Fixture) =>
	page.getByRole('button', { name: new RegExp(`^(?:T-[0-9]+ · )?${escapeRegex(f.title)}$`) });
export function sheet(page: Page) {
	return page.getByRole('dialog');
}
export function fact(view: Locator, label: string) {
	return view
		.locator('dt')
		.filter({ hasText: new RegExp(`^${escapeRegex(label)}$`) })
		.locator('xpath=following-sibling::dd[1]');
}
export async function assertPair(page: Page, f: Fixture, desired: Pair) {
	await expect(fact(sheet(page), 'Accountable team')).toHaveText(f.accountableTeamId);
	await expect(fact(sheet(page), 'Assigned team')).toHaveText(desired.assigned_team_id ?? 'None');
	await expect(fact(sheet(page), 'Assignee')).toHaveText(desired.assignee_id ?? 'None');
}
export async function choose(page: Page, desired: Pair) {
	await sheet(page)
		.getByLabel('Assigned team', { exact: true })
		.selectOption(desired.assigned_team_id ?? '');
	await sheet(page)
		.getByLabel('Assignee', { exact: true })
		.selectOption(desired.assignee_id ?? '');
}
type ObservedRequests = {
	intents: Intent[];
	lookups: string[];
	readPaths: string[];
	proof: string;
};
function recordConsoleError(
	message: ConsoleMessage,
	f: Fixture,
	errors: string[],
	expectedNetworkErrors: Set<string>,
	unavailableLookupURLs: Set<string>
) {
	const text = message.text();
	if (/Content Security Policy|Refused to (execute|apply|load)/i.test(text)) {
		errors.push('CSP console violation');
		return;
	}
	if (message.type() !== 'error') return;
	const url = message.location().url;
	if (
		unavailableLookupURLs.has(url) &&
		/^Failed to load resource: the server responded with a status of 503(?:\s|$)/.test(text)
	)
		return;
	const expected = [...expectedNetworkErrors].some((path) => url === f.origin + path);
	if (
		expected &&
		/Failed to load resource:|Failed to fetch|Load failed/i.test(text) &&
		/401|403|404|409|ERR_FAILED|network connection was lost|Load failed/i.test(text)
	)
		return;
	errors.push('unexpected console error');
}
function recordAuthorizedRequest(request: Request, f: Fixture, observed: ObservedRequests) {
	const url = new URL(request.url());
	if (url.origin !== f.origin || !url.pathname.startsWith('/api/v1/')) return;
	const header = request.headers()['x-org-session-proof'];
	if (header) {
		observed.proof = header;
		rememberSecret(header);
	}
	if (request.method() === 'POST' && url.pathname === assignmentPath(f))
		observed.intents.push(request.postDataJSON() as Intent);
	if (request.method() === 'GET' && url.pathname.startsWith('/api/v1/operations/work/'))
		observed.lookups.push(url.pathname + url.search);
	if (request.method() === 'GET' && url.pathname.startsWith(taskBase(f)))
		observed.readPaths.push(url.pathname + url.search);
}
export function monitor(page: Page, f: Fixture) {
	const errors: string[] = [];
	const observed: ObservedRequests = { intents: [], lookups: [], readPaths: [], proof: '' };
	const { intents, lookups, readPaths } = observed;
	const expectedNetworkErrors = new Set<string>();
	const unavailableLookupURLs = new Set<string>();
	page.on('pageerror', () => errors.push('uncaught client error'));
	page.on('console', (message) =>
		recordConsoleError(message, f, errors, expectedNetworkErrors, unavailableLookupURLs)
	);
	page.on('request', (request: Request) => recordAuthorizedRequest(request, f, observed));
	return {
		errors,
		intents,
		lookups,
		readPaths,
		get proof() {
			if (!observed.proof) throw new Error('No actual authorized browser request was observed');
			return observed.proof;
		},
		allowNetworkFailure(path: string) {
			expectedNetworkErrors.add(path);
		},
		allowUnavailableLookups(paths: readonly [string, string]) {
			if (unavailableLookupURLs.size || paths[0] === paths[1])
				throw new Error('Exactly two distinct unavailable lookup URLs may be registered once');
			for (const path of paths) unavailableLookupURLs.add(f.origin + path);
		}
	};
}
export async function watchCsp(page: Page) {
	await page.addInitScript(() => {
		const state = window as Window & { selectedCspViolations?: number };
		state.selectedCspViolations = 0;
		window.addEventListener('securitypolicyviolation', () => state.selectedCspViolations!++);
	});
}
export async function assertNoCsp(page: Page) {
	expect(
		await page.evaluate(
			() => (window as Window & { selectedCspViolations?: number }).selectedCspViolations
		)
	).toBe(0);
}
export async function enterThroughNavigation(page: Page, f: Fixture) {
	const response = await page.goto(f.origin);
	expect(response?.status()).toBe(200);
	const csp = response?.headers()['content-security-policy'] ?? '';
	expect(csp).toMatch(/(?:^|;)\s*default-src\s+'(?:none|self)'(?:;|$)/);
	expect(csp).toContain("object-src 'none'");
	expect(csp).not.toContain("'unsafe-eval'");
	const tasks = page.getByRole('link', { name: 'Tasks', exact: true });
	await tasks.focus();
	await expect(tasks).toBeFocused();
	await page.keyboard.press('Enter');
	await expect(page).toHaveURL(`${f.origin}/tasks`);
	await expect(page.getByRole('heading', { name: 'Tasks', exact: true })).toBeVisible();
	await expect(page).toHaveTitle('Tasks · Agentic Org');
}
export async function signIn(page: Page, f: Credentials): Promise<SessionIdentity> {
	rememberSecret(f.login);
	rememberSecret(f.password);
	await expect(page.getByRole('button', { name: 'Sign in', exact: true })).toBeEnabled();
	for (const [label, value] of [
		['Login', f.login],
		['Password', f.password]
	]) {
		// Fill arguments and failure call logs must not contain generated credentials.
		await page.getByLabel(label, { exact: true }).evaluate((input: HTMLInputElement, secret) => {
			input.value = secret;
			input.dispatchEvent(new Event('input', { bubbles: true }));
		}, value);
	}
	const response = page.waitForResponse(
		(reply) =>
			reply.url() === new URL('/api/v1/session/sign-in', page.url()).href &&
			reply.request().method() === 'POST'
	);
	await page.getByRole('button', { name: 'Sign in', exact: true }).click();
	const actual = await response;
	expect(actual.status()).toBe(200);
	const session = object(object(await actual.json()).result);
	const proof = required(session.proof);
	rememberSecret(proof);
	const identity = {
		accountId: required(object(session.account).id),
		instanceId: required(session.instance_id),
		proof
	};
	await expect(page.getByRole('button', { name: 'Sign out', exact: true })).toBeVisible();
	await expect(page.getByLabel('Password', { exact: true })).toHaveCount(0);
	return identity;
}
export async function queue(page: Page, f: Fixture, kind = 'all', target?: string) {
	await page.getByLabel('Organization', { exact: true }).selectOption(f.organizationId);
	await page.getByLabel('Office', { exact: true }).selectOption(f.officeId);
	await page.getByLabel('Queue', { exact: true }).selectOption(kind);
	if (target)
		await page
			.getByLabel(kind === 'assigned_employee' ? 'Employee ID' : 'Team ID', { exact: true })
			.fill(target);
	const response = page.waitForResponse((r) => {
		const u = new URL(r.url());
		return (
			u.origin === f.origin &&
			u.pathname === taskBase(f).slice(0, taskBase(f).lastIndexOf('/')) &&
			u.searchParams.get('kind') === 'list' &&
			u.searchParams.get('queue') === kind
		);
	});
	await page.getByRole('button', { name: 'Open queue', exact: true }).click();
	const actual = await response;
	expect(actual.status()).toBe(200);
	await expect(page.getByRole('heading', { name: 'Task queue', exact: true })).toBeVisible();
	return object(object(await actual.json()).result);
}
export async function assertContainment(page: Page) {
	const measureGeometry = () =>
		sheet(page).evaluate((element) => {
			const rectangle = (item: Element) => {
				const box = item.getBoundingClientRect();
				return { left: box.left, right: box.right, width: box.width };
			};
			return {
				viewportWidth: innerWidth,
				dialog: rectangle(element),
				clientWidth: element.clientWidth,
				scrollWidth: element.scrollWidth,
				controls: [...element.querySelectorAll('fieldset, select')].map((item) => ({
					name: item.id || item.tagName.toLowerCase(),
					...rectangle(item)
				}))
			};
		});
	let geometry!: Awaited<ReturnType<typeof measureGeometry>>;
	let previous: string | undefined;
	try {
		await expect
			.poll(
				async () => {
					geometry = await measureGeometry();
					const serialized = JSON.stringify(geometry);
					// The first sample is unstable; equality requires a separate poll attempt.
					const stable = previous !== undefined && serialized === previous;
					previous = serialized;
					return { stable, geometry };
				},
				{ message: 'Assignment dialog geometry must settle before containment checks' }
			)
			.toMatchObject({ stable: true });
	} catch (cause) {
		throw new Error(
			`Assignment dialog geometry did not settle; last sample: ${JSON.stringify(geometry)}`,
			{ cause }
		);
	}
	expect(geometry.dialog.left).toBeGreaterThanOrEqual(-1);
	expect(geometry.dialog.right).toBeLessThanOrEqual(geometry.viewportWidth + 1);
	expect(
		geometry.scrollWidth,
		'Assignment dialog must not require horizontal scrolling'
	).toBeLessThanOrEqual(geometry.clientWidth + 1);
	expect(geometry.controls.length).toBeGreaterThanOrEqual(3);
	for (const control of geometry.controls) {
		expect(control.left, `${control.name} must stay inside the dialog`).toBeGreaterThanOrEqual(
			geometry.dialog.left - 1
		);
		expect(control.right, `${control.name} must stay inside the dialog`).toBeLessThanOrEqual(
			geometry.dialog.right + 1
		);
	}
}
export async function open(page: Page, f: Fixture, revision: bigint) {
	await taskButton(page, f).focus();
	await expect(taskButton(page, f)).toBeFocused();
	await page.keyboard.press('Enter');
	await expect(sheet(page)).toBeVisible();
	await expect(sheet(page)).toHaveAccessibleName(f.title);
	await expect(
		sheet(page).getByText(`${f.taskId} · Revision ${revision}`, { exact: true })
	).toBeVisible();
	await expect(
		sheet(page).getByRole('button', { name: 'Save assignment', exact: true })
	).toBeEnabled();
	expect(await sheet(page).evaluate((element) => element.contains(document.activeElement))).toBe(
		true
	);
	await assertContainment(page);
	for (const key of ['Shift+Tab', 'Tab']) {
		await page.keyboard.press(key);
		expect(await sheet(page).evaluate((element) => element.contains(document.activeElement))).toBe(
			true
		);
	}
}
export async function close(page: Page, f: Fixture) {
	await page.keyboard.press('Escape');
	await expect(sheet(page)).toHaveCount(0);
	await expect(taskButton(page, f)).toBeFocused();
}
export async function acknowledge(page: Page) {
	await sheet(page)
		.getByRole('button', { name: 'Acknowledge result and reload for a new edit', exact: true })
		.click();
	await expect(
		sheet(page).getByRole('button', { name: 'Save assignment', exact: true })
	).toBeEnabled();
	await expect(sheet(page).getByText(/^Operation /)).toHaveCount(0);
}
export async function operationId(page: Page) {
	const line = await sheet(page)
		.getByText(/^Operation [0-7][0-9A-HJKMNP-TV-Z]{25}$/)
		.innerText();
	return line.slice('Operation '.length);
}
export async function realJson(
	page: Page,
	path: string,
	proof?: string,
	body?: unknown
): Promise<Reply> {
	return page.evaluate(
		async ({ path, proof, body }) => {
			const response = await fetch(path, {
				method: body === undefined ? 'GET' : 'POST',
				credentials: 'same-origin',
				cache: 'no-store',
				headers: {
					Accept: 'application/json',
					...(proof ? { 'X-Org-Session-Proof': proof } : {}),
					...(body === undefined ? {} : { 'Content-Type': 'application/json' })
				},
				...(body === undefined ? {} : { body: JSON.stringify(body) })
			});
			return { status: response.status, body: await response.json() };
		},
		{ path, proof, body }
	);
}
export async function publicScreenshot(page: Page, f: Fixture, info: TestInfo, name: string) {
	await page.screenshot({
		path: info.outputPath(name),
		fullPage: true,
		mask: [
			page.getByText(f.login, { exact: true }),
			page.getByText(f.foreignAccount.login, { exact: true }),
			page.locator('input')
		]
	});
}
export function normalizedSnapshot(source: string) {
	const start = /^\uFEFF?---\r?\n/.exec(source);
	if (!start) throw new Error('Fixture public snapshot must have a Markdown front matter fence');
	const rest = source.slice(start[0].length);
	const end = /^---[ \t]*\r?$/m.exec(rest);
	if (!end) throw new Error('Missing public snapshot closing fence');
	let header = rest.slice(0, end.index);
	for (const key of [
		'revision',
		'assigned_team_id',
		'assignee_id',
		'updated_at',
		'updated_by_kind',
		'updated_by_id'
	]) {
		let count = 0;
		header = header.replace(
			new RegExp(
				`^(${key}:[ \\t]*)("(?:[^"\\\\]|\\\\.)*"|'(?:[^']|'')*'|[^\\s#]+)([ \\t]*(?:#.*)?\\r?)$`,
				'gm'
			),
			(_all, prefix: string, _value: string, suffix: string) => {
				count++;
				return `${prefix}<accepted mutable value>${suffix}`;
			}
		);
		if (count !== 1)
			throw new Error('Fixture requires one ordinary scalar for each mutable root key');
	}
	return start[0] + header + rest.slice(end.index);
}
export async function readSnapshot(page: Page, f: Fixture, reference: PublicReference) {
	const chunks: Promise<{ status: number; body: unknown; offset: string | null }>[] = [];
	const collect = (response: import('@playwright/test').Response) => {
		const u = new URL(response.url());
		if (
			u.origin === f.origin &&
			u.pathname === `${taskBase(f)}/content` &&
			u.searchParams.get('kind') === 'snapshot'
		)
			chunks.push(
				response.json().then((body: unknown) => ({
					status: response.status(),
					body,
					offset: u.searchParams.get('offset')
				}))
			);
	};
	page.on('response', collect);
	try {
		await sheet(page)
			.getByRole('button', { name: 'View verified public Markdown', exact: true })
			.click();
		const region = sheet(page).getByRole('region', { name: 'Verified public source', exact: true });
		await expect(region.getByRole('status')).toHaveText('Verified public task Markdown.');
		const source = (await region.locator('pre').textContent()) ?? '';
		const actual = await Promise.all(chunks);
		expect(reference.byte_length).toBeGreaterThan(256 * 1024);
		expect(actual.length).toBeGreaterThan(1);
		let offset = 0;
		const bytes: Buffer[] = [];
		for (const response of actual) {
			expect(response.status).toBe(200);
			const chunk = object(object(response.body).result);
			expect(chunk.kind).toBe('chunk');
			expect(chunk.reference).toEqual(reference);
			expect(chunk.offset).toBe(offset);
			expect(response.offset).toBe(String(offset));
			expect(typeof chunk.hex).toBe('string');
			expect(chunk.hex).toMatch(/^(?:[0-9a-f]{2})+$/);
			const raw = Buffer.from(chunk.hex as string, 'hex');
			expect(raw.byteLength).toBeGreaterThan(0);
			expect(raw.byteLength).toBeLessThanOrEqual(256 * 1024);
			bytes.push(raw);
			offset += raw.byteLength;
			expect(chunk.next).toBe(offset === reference.byte_length ? null : offset);
		}
		const original = Buffer.concat(bytes);
		expect(original.byteLength).toBe(reference.byte_length);
		expect(createHash('sha256').update(original).digest('hex')).toBe(reference.sha256);
		expect(source).toBe(original.toString('utf8'));
		expect(source).toContain(f.largeBodyTail);
		expect(source).toContain('1.234567890123456789E+012');
		expect(source).toMatch(
			new RegExp(`^revision:[ \\t]*${reference.revision}(?:[ \\t#]|\\r?$)`, 'm')
		);
		return source;
	} finally {
		page.off('response', collect);
	}
}
export function deferred<T = void>() {
	let resolve!: (value: T | PromiseLike<T>) => void;
	let reject!: (reason?: unknown) => void;
	const promise = new Promise<T>((yes, no) => {
		resolve = yes;
		reject = no;
	});
	return { promise, resolve, reject };
}
