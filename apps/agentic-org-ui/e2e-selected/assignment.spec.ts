import { expect, test, type BrowserContext, type Page, type Route } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
import { randomBytes } from 'node:crypto';
import { isDeepStrictEqual } from 'node:util';
import {
	acknowledge,
	asObject,
	assertNoCsp,
	assertPair,
	assignmentPath,
	choose,
	close,
	deferred,
	enterThroughNavigation,
	fixture,
	monitor,
	normalizedSnapshot,
	open,
	operationId,
	publicScreenshot,
	queue,
	readSnapshot,
	realJson,
	rememberSecret,
	revokeOwnerAccount,
	sanitizeErrors,
	sheet,
	signIn,
	taskBase,
	taskButton,
	watchCsp,
	type Intent,
	type Pair,
	type PublicReference,
	type Reply,
	type SessionIdentity
} from './support';

test.afterEach(({ browserName }, info) => {
	sanitizeErrors(info);
	info.annotations.push({ type: 'browser', description: browserName });
});

test('real protected assignment, history, conflict, recovery and session journey @journey @webkit', async ({
	page,
	browserName,
	browser
}, info) => {
	const f = fixture(browserName);
	const observed = monitor(page, f);
	const peerOperationIds = new Set<string>();
	await watchCsp(page);
	let revision = BigInt(f.initialRevision);
	let current!: Pair;
	let originalSource = '';
	let originalNormalized = '';
	let ownerIdentity!: SessionIdentity;
	const none: Pair = { assigned_team_id: null, assignee_id: null };
	const both: Pair = { assigned_team_id: f.assignedTeamId, assignee_id: f.employeeId };
	const teamOnly: Pair = { assigned_team_id: f.assignedTeamId, assignee_id: null };
	const employeeOnly: Pair = { assigned_team_id: null, assignee_id: f.employeeId };
	const states = [
		['unassigned', none],
		['team only', teamOnly],
		['employee only', employeeOnly],
		['both', both]
	] as const;
	const taskReply = async (view = page) => {
		const path = observed.readPaths.find(
			(path) => new URL(path, f.origin).pathname === taskBase(f)
		);
		if (!path) throw new Error('Actual task detail request was not observed');
		const reply = await realJson(view, path, observed.proof);
		expect(reply.status).toBe(200);
		return asObject(asObject(reply.body.result).task);
	};
	const resultOf = async (response: {
		status(): number;
		json(): Promise<unknown>;
	}): Promise<Reply> => ({
		status: response.status(),
		body: asObject(await response.json())
	});
	const assignmentResponse = (view: Page) =>
		view.waitForResponse(
			(response) =>
				response.url() === f.origin + assignmentPath(f) && response.request().method() === 'POST'
		);
	const expectIntent = (intent: Intent, expected: bigint, desired: Pair) => {
		expect(Object.keys(intent).sort()).toEqual(['operation_id', 'params', 'scope']);
		expect(Object.keys(intent.params).sort()).toEqual([
			'assigned_team_id',
			'assignee_id',
			'expected_revision'
		]);
		expect(intent.operation_id).toMatch(/^[0-7][0-9A-HJKMNP-TV-Z]{25}$/);
		expect(intent.scope.kind).toBe('office');
		expect(intent.scope.org_id).toBe(f.organizationId);
		expect(intent.scope.office_id).toBe(f.officeId);
		expect(intent.params).toEqual({ expected_revision: String(expected), ...desired });
	};
	const expectCompleted = (reply: Reply, expected: bigint, desired: Pair) => {
		expect(reply.status).toBe(200);
		expect(reply.body.outcome).toBe('completed');
		const result = asObject(reply.body.result);
		expect(result.status).toBe('completed');
		expect(result.revision).toBe(String(expected));
		expect(result.organization_id).toBe(f.organizationId);
		expect(result.office_id).toBe(f.officeId);
		expect(result.task_id).toBe(f.taskId);
		expect(result.assigned_team_id).toBe(desired.assigned_team_id);
		expect(result.assignee_id).toBe(desired.assignee_id);
		expect(result.event_sha256).toMatch(/^[0-9a-f]{64}$/);
		return result;
	};
	const readAssignmentEvent = async (view: Page, before: Pair, after: Pair, at: bigint) => {
		await sheet(view)
			.getByRole('button', { name: `Read event at revision ${at}`, exact: true })
			.click();
		const source = sheet(view).getByRole('region', { name: 'Verified public source', exact: true });
		const label = (pair: Pair) =>
			`team ${pair.assigned_team_id ?? 'None'}, assignee ${pair.assignee_id ?? 'None'}`;
		await expect(source.getByRole('status')).toHaveText(
			`Revision ${at}. Assignment changed from ${label(before)} to ${label(after)}. Actor: owner.`
		);
		const event = asObject(JSON.parse((await source.locator('pre').textContent()) ?? ''));
		expect(event.actor_kind).toBe('owner');
		expect(event.actor_id).toBeNull();
		expect(asObject(event.change).type).toBe('assignment_changed');
		expect(asObject(event.change).from).toEqual(before);
		expect(asObject(event.change).to).toEqual(after);
		// Numeric JSON fields are deliberately never used as browser revisions.
	};
	const save = async (view: Page, before: Pair, desired: Pair, select = true) => {
		if (select) await choose(view, desired);
		const expected = revision;
		const response = assignmentResponse(view);
		await sheet(view).getByRole('button', { name: 'Save assignment', exact: true }).click();
		const actual = await response;
		const intent = actual.request().postDataJSON() as Intent;
		expectIntent(intent, expected, desired);
		revision++;
		const result = expectCompleted(await resultOf(actual), revision, desired);
		await expect(
			sheet(view).getByRole('status').filter({ hasText: 'Assignment saved' })
		).toContainText(`Assignment saved at revision ${revision}.`);
		expect(await operationId(view)).toBe(intent.operation_id);
		await expect(
			sheet(view).getByRole('button', { name: 'Save assignment', exact: true })
		).toBeDisabled();
		await assertPair(view, f, desired);
		await readAssignmentEvent(view, before, desired, revision);
		await acknowledge(view);
		current = desired;
		return { intent, result };
	};

	await test.step('normal navigation, real sign-in, accessible sheet and authenticated large original source', async () => {
		await enterThroughNavigation(page, f);
		ownerIdentity = await signIn(page, f);
		await queue(page, f);
		await open(page, f, revision);
		const task = await taskReply();
		expect(task.task_id).toBe(f.taskId);
		expect(task.revision).toBe(f.initialRevision);
		expect(task.accountable_team_id).toBe(f.accountableTeamId);
		current = {
			assigned_team_id: task.assigned_team_id as string | null,
			assignee_id: task.assignee_id as string | null
		};
		await assertPair(page, f, current);
		originalSource = await readSnapshot(page, f, task.snapshot as PublicReference);
		originalNormalized = normalizedSnapshot(originalSource);
		const detailPath = observed.readPaths.find(
			(path) => new URL(path, f.origin).pathname === taskBase(f)
		);
		if (!detailPath) throw new Error('Actual task detail request was not observed');
		const detail = asObject((await realJson(page, detailPath, observed.proof)).body.result);
		await expect(sheet(page).getByRole('region', { name: 'Task history' })).toContainText(
			`Coverage: ${detail.history_coverage}.`
		);
		const accessibility = await new AxeBuilder({ page }).include('[role="dialog"]').analyze();
		expect(accessibility.violations.map((violation) => violation.id)).toEqual([]);
		await publicScreenshot(page, f, info, 'assignment-original-source.png');
		await close(page, f);
		await open(page, f, revision);
	});

	await test.step('truthful imported baseline and unknown future event retain original public history', async () => {
		const history = sheet(page).getByRole('region', { name: 'Task history', exact: true });
		await expect(history).toContainText('Earlier history may be unavailable or unverified.');
		const entries = await history
			.getByRole('button', { name: /^Read event at revision / })
			.allTextContents();
		expect(entries.length).toBeGreaterThanOrEqual(2);
		let baseline = false;
		let unknown = false;
		for (const name of entries) {
			await history.getByRole('button', { name, exact: true }).click();
			const source = sheet(page).getByRole('region', {
				name: 'Verified public source',
				exact: true
			});
			await expect(source.getByRole('status')).toContainText(`Revision ${name.split(' ').at(-1)}.`);
			const summary = await source.getByRole('status').innerText();
			const text = (await source.locator('pre').textContent()) ?? '';
			const change = asObject(asObject(JSON.parse(text)).change);
			if (change.type === 'history_baseline') {
				baseline = true;
				expect(summary).toContain('Imported snapshot baseline. Earlier history is unavailable.');
				const retained = asObject(change.public_snapshot).text;
				expect(typeof retained).toBe('string');
				expect(retained).toContain('1.234567890123456789E+012');
				expect(normalizedSnapshot(retained as string)).toBe(originalNormalized);
			}
			if (change.type === 'browser_fixture_future_change') {
				unknown = true;
				expect(summary).toContain(
					'Unknown activity (browser_fixture_future_change). Original event retained.'
				);
				expect(change.opaque).toEqual({ retained: 'future public activity payload' });
				expect(text).toContain('future public activity payload');
			}
		}
		expect(baseline, 'Actual adoption must retain its truthful baseline').toBe(true);
		expect(unknown, 'Actual adoption must retain the agreed future public change').toBe(true);
	});

	for (const [fromName, from] of states) {
		for (const [toName, to] of states) {
			await test.step(`${fromName} → ${toName}: complete pair, exact revision, immutable attributed event`, async () => {
				await save(page, current, from);
				await save(page, from, to);
			});
		}
	}

	await test.step('each clear action persists independently, including cross-home assignment', async () => {
		await save(page, current, both);
		await sheet(page).getByRole('button', { name: 'Clear assigned team', exact: true }).click();
		await expect(sheet(page).getByLabel('Assigned team', { exact: true })).toHaveValue('');
		await expect(sheet(page).getByLabel('Assignee', { exact: true })).toHaveValue(f.employeeId);
		await save(page, both, employeeOnly, false);
		await save(page, current, both);
		await sheet(page).getByRole('button', { name: 'Clear assignee', exact: true }).click();
		await expect(sheet(page).getByLabel('Assignee', { exact: true })).toHaveValue('');
		await expect(sheet(page).getByLabel('Assigned team', { exact: true })).toHaveValue(
			f.assignedTeamId
		);
		await save(page, both, teamOnly, false);
		const task = await taskReply();
		expect(normalizedSnapshot(await readSnapshot(page, f, task.snapshot as PublicReference))).toBe(
			originalNormalized
		);
		await save(page, current, both);
	});

	await test.step('owner backlog and independent assignment queues show and exclude the actual task', async () => {
		await close(page, f);
		for (const [kind, target, included] of [
			['owned', f.accountableTeamId, true],
			['owned', f.assignedTeamId, false],
			['assigned_team', f.assignedTeamId, true],
			['assigned_team', f.accountableTeamId, false],
			['assigned_employee', f.employeeId, true]
		] as const) {
			const result = await queue(page, f, kind, target);
			const items = result.items as Record<string, unknown>[];
			expect(items.some((item) => item.task_id === f.taskId)).toBe(included);
			if (included) {
				await expect(taskButton(page, f)).toBeVisible();
				const row = taskButton(page, f).locator('xpath=ancestor::li[1]');
				await expect(row).toContainText(`Accountable: ${f.accountableTeamId}`);
				await expect(row).toContainText(`Assigned team: ${f.assignedTeamId}`);
				await expect(row).toContainText(`Assignee: ${f.employeeId}`);
			} else await expect(taskButton(page, f)).toHaveCount(0);
		}
		await publicScreenshot(page, f, info, 'assignment-employee-queue.png');
		await queue(page, f);
		await open(page, f, revision);
	});

	await test.step('two real concurrent writes accept once, preserve the losing draft and require a new intent', async () => {
		const peer = await page.context().newPage();
		const peerObserved = monitor(peer, f);
		await watchCsp(peer);
		const release = deferred();
		let arrivals = 0;
		const race = async (route: Route) => {
			arrivals++;
			await release.promise;
			await route.continue();
		};
		const routeUrl = f.origin + assignmentPath(f);
		try {
			await enterThroughNavigation(peer, f);
			await expect(peer.getByRole('button', { name: 'Sign out', exact: true })).toBeVisible();
			await queue(peer, f);
			await open(peer, f, revision);
			await choose(page, none);
			await choose(peer, teamOnly);
			observed.allowNetworkFailure(assignmentPath(f));
			peerObserved.allowNetworkFailure(assignmentPath(f));
			const attemptsBeforeRace = observed.intents.length + peerObserved.intents.length;
			await page.context().route(routeUrl, race);
			const replies = [assignmentResponse(page), assignmentResponse(peer)];
			await Promise.all(
				[page, peer].map((view) =>
					sheet(view).getByRole('button', { name: 'Save assignment', exact: true }).click()
				)
			);
			await expect
				.poll(() => arrivals, { message: 'Both actual writes must reach the barrier' })
				.toBe(2);
			for (const view of [page, peer]) {
				await expect(
					sheet(view).getByRole('status').filter({ hasText: 'Waiting for the office' })
				).toBeVisible();
				await expect(
					sheet(view).getByRole('button', { name: 'Save assignment', exact: true })
				).toBeDisabled();
				await expect(sheet(view).getByLabel('Assigned team', { exact: true })).toBeDisabled();
			}
			release.resolve();
			const responses = await Promise.all(replies);
			const actual = await Promise.all(responses.map(resultOf));
			expect(actual.map((reply) => reply.status).sort()).toEqual([200, 409]);
			const winner = actual[0].status === 200 ? 0 : 1;
			const loser = 1 - winner;
			const views = [page, peer];
			const desired = [none, teamOnly];
			for (let index = 0; index < 2; index++)
				expectIntent(responses[index].request().postDataJSON() as Intent, revision, desired[index]);
			revision++;
			expectCompleted(actual[winner], revision, desired[winner]);
			expect(actual[loser].body.outcome).toBe('rejected');
			const refusal = asObject(actual[loser].body.error);
			expect(refusal.code).toBe('conflict');
			expect(asObject(refusal.current).revision).toBe(String(revision));
			expect(asObject(refusal.current).assigned_team_id).toBe(desired[winner].assigned_team_id);
			expect(asObject(refusal.current).assignee_id).toBe(desired[winner].assignee_id);
			await expect(
				sheet(views[loser]).getByRole('status').filter({ hasText: 'Your edit was not applied' })
			).toBeVisible();
			await expect(
				sheet(views[loser]).getByText('State observed at conflict', { exact: true })
			).toBeVisible();
			await expect(
				sheet(views[loser]).getByText(new RegExp(`Revision ${revision}; team`))
			).toContainText('This is the original receipt’s state.');
			const losingOperation = await operationId(views[loser]);
			await publicScreenshot(views[loser], f, info, 'assignment-conflict.png');
			await sheet(views[loser])
				.getByRole('button', { name: 'Check original operation', exact: true })
				.click();
			expect(await operationId(views[loser])).toBe(losingOperation);
			await readAssignmentEvent(views[winner], current, desired[winner], revision);
			await acknowledge(views[winner]);
			await acknowledge(views[loser]);
			await expect(sheet(views[loser]).getByLabel('Assigned team', { exact: true })).toHaveValue(
				desired[loser].assigned_team_id ?? ''
			);
			await expect(sheet(views[loser]).getByLabel('Assignee', { exact: true })).toHaveValue(
				desired[loser].assignee_id ?? ''
			);
			const reapplied = await save(views[loser], desired[winner], desired[loser], false);
			expect(reapplied.intent.operation_id).not.toBe(losingOperation);
			expect(observed.intents.length + peerObserved.intents.length).toBe(attemptsBeforeRace + 3);
			await assertNoCsp(peer);
			expect(peerObserved.errors).toEqual([]);
		} finally {
			for (const intent of peerObserved.intents) peerOperationIds.add(intent.operation_id);
			release.resolve();
			await page.context().unroute(routeUrl, race);
			await peer.close();
		}
		// The other tab's successful new intent invalidates this tab's old detail.
		await close(page, f);
		await queue(page, f);
		await open(page, f, revision);
		await assertPair(page, f, current);
	});

	let lostIntent: Intent;
	let originalCompletion: Record<string, unknown>;
	let originalOperation = '';
	await test.step('actual accepted reply loss stays unknown and reload looks up only the original operation', async () => {
		const delivered = deferred<Reply>();
		const drop = deferred();
		const lostResponse = async (route: Route) => {
			try {
				lostIntent = route.request().postDataJSON() as Intent;
				const response = await route.fetch();
				delivered.resolve(await resultOf(response));
				await drop.promise;
				await route.abort('failed');
			} catch (error) {
				delivered.reject(error);
				await route.abort('failed');
			}
		};
		const routeUrl = f.origin + assignmentPath(f);
		const count = observed.intents.length;
		await choose(page, employeeOnly);
		await page.route(routeUrl, lostResponse);
		try {
			await sheet(page).getByRole('button', { name: 'Save assignment', exact: true }).click();
			const accepted = await delivered.promise;
			expectIntent(lostIntent!, revision, employeeOnly);
			revision++;
			originalCompletion = expectCompleted(accepted, revision, employeeOnly);
			originalOperation = await operationId(page);
			expect(originalOperation).toBe(lostIntent!.operation_id);
			await expect(
				sheet(page).getByRole('status').filter({ hasText: 'Waiting for the office' })
			).toBeVisible();
			await expect(
				sheet(page).getByRole('button', { name: 'Save assignment', exact: true })
			).toBeDisabled();
			await publicScreenshot(page, f, info, 'assignment-awaiting-real-reply.png');
			drop.resolve();
			await expect(
				sheet(page).getByRole('status').filter({ hasText: 'outcome is not yet known' })
			).toBeVisible();
			await expect(
				sheet(page).getByRole('button', { name: 'Save assignment', exact: true })
			).toBeDisabled();
			await expect(
				sheet(page).getByRole('button', {
					name: 'Acknowledge result and reload for a new edit',
					exact: true
				})
			).toHaveCount(0);
			await publicScreenshot(page, f, info, 'assignment-unknown.png');
		} finally {
			drop.resolve();
			await page.unroute(routeUrl, lostResponse);
		}
		const lookupStart = observed.lookups.length;
		await page.reload();
		await expect(page.getByRole('button', { name: 'Sign out', exact: true })).toBeVisible();
		await queue(page, f);
		await taskButton(page, f).click();
		await expect(
			sheet(page).getByRole('status').filter({ hasText: 'Assignment saved' })
		).toBeVisible();
		expect(await operationId(page)).toBe(originalOperation);
		await expect(
			sheet(page).getByRole('status').filter({ hasText: 'Assignment saved' })
		).toContainText(`revision ${revision}.`);
		expect(observed.intents.length).toBe(count + 1);
		const resumed = observed.lookups.slice(lookupStart);
		expect(resumed.length).toBeGreaterThan(0);
		for (const path of resumed) {
			const url = new URL(path, f.origin);
			expect(url.pathname).toBe(`/api/v1/operations/work/${originalOperation}`);
			expect(url.searchParams.get('instance_id')).toBe(lostIntent!.scope.instance_id);
			expect(url.searchParams.get('org_id')).toBe(f.organizationId);
			expect(url.searchParams.get('office_id')).toBe(f.officeId);
		}
		await assertPair(page, f, employeeOnly);
		await readAssignmentEvent(page, current, employeeOnly, revision);
		current = employeeOnly;
		await acknowledge(page);
	});

	await test.step('later changes preserve original lookup/replay results and untouched snapshot bytes', async () => {
		await save(page, current, both);
		const before = await taskReply();
		const lookup = observed.lookups.find((path) => path.includes(originalOperation));
		if (!lookup) throw new Error('Original actual receipt lookup was not observed');
		const found = await realJson(page, lookup, observed.proof);
		expect(found.status).toBe(200);
		const stored = asObject(found.body.result);
		expect(stored.operation_id).toBe(originalOperation);
		expect(stored.outcome).toBe('completed');
		expect(stored.result).toEqual(originalCompletion!);
		const replay = await realJson(page, assignmentPath(f), observed.proof, lostIntent!);
		expect(replay.status).toBe(200);
		expect(replay.body.operation_id).toBe(originalOperation);
		expect(replay.body.result).toEqual(originalCompletion!);
		const after = await taskReply();
		expect(after).toEqual(before);
		expect(after.revision).toBe(String(revision));
		const finalSource = await readSnapshot(page, f, after.snapshot as PublicReference);
		expect(finalSource).not.toBe(originalSource);
		expect(normalizedSnapshot(finalSource)).toBe(originalNormalized);
		await readAssignmentEvent(page, employeeOnly, both, revision);
		const accessibility = await new AxeBuilder({ page }).include('[role="dialog"]').analyze();
		expect(accessibility.violations.map((violation) => violation.id)).toEqual([]);
		await publicScreenshot(page, f, info, 'assignment-history.png');
	});

	await test.step('anonymous reads, chunks, targets, mutations and lookup reveal no protected task result', async () => {
		const unauth = await browser.newContext();
		try {
			const anonymous = await unauth.newPage();
			await anonymous.goto(f.origin + '/tasks');
			const paths = [
				observed.readPaths.find((path) => new URL(path, f.origin).pathname === taskBase(f)),
				observed.readPaths.find((path) => new URL(path, f.origin).pathname.endsWith('/history')),
				observed.readPaths.find((path) =>
					new URL(path, f.origin).pathname.endsWith('/assignment-targets')
				),
				observed.readPaths.find((path) => new URL(path, f.origin).pathname.endsWith('/content')),
				observed.lookups.find((path) => path.includes(originalOperation))
			];
			for (const path of paths) {
				if (!path) throw new Error('Required actual protected read was not observed');
				const denied = await realJson(anonymous, path);
				expect(denied.status).toBe(401);
				expect(asObject(denied.body.error).code).toBe('not_authorized');
				expect(denied.body).not.toHaveProperty('result');
			}
			const denied = await realJson(anonymous, assignmentPath(f), undefined, lostIntent!);
			expect(denied.status).toBe(401);
			expect(asObject(denied.body.error).code).toBe('not_authorized');
			expect(denied.body).not.toHaveProperty('result');
		} finally {
			await unauth.close();
		}
		const path = observed.lookups.find((path) => path.includes(originalOperation));
		if (!path) throw new Error('Original actual lookup was not observed');
		const wrongScope = new URL(path, f.origin);
		const wrongOffice = f.officeId.slice(0, -1) + (f.officeId.endsWith('0') ? '1' : '0');
		wrongScope.searchParams.set('office_id', wrongOffice);
		observed.allowNetworkFailure(wrongScope.pathname + wrongScope.search);
		const denied = await realJson(page, wrongScope.pathname + wrongScope.search, observed.proof);
		expect([403, 404]).toContain(denied.status);
		expect(asObject(denied.body.error).code).toBe(
			denied.status === 403 ? 'not_authorized' : 'not_found'
		);
		expect(denied.body).not.toHaveProperty('result');
		const content = observed.readPaths.find((path) =>
			new URL(path, f.origin).pathname.endsWith('/content')
		);
		if (!content) throw new Error('Actual task-scoped chunk read was not observed');
		const otherTaskContent = content.replace(
			taskBase(f),
			taskBase(f).replace(encodeURIComponent(f.taskId), encodeURIComponent(f.otherTaskId))
		);
		observed.allowNetworkFailure(otherTaskContent);
		const wrongTask = await realJson(page, otherTaskContent, observed.proof);
		expect(wrongTask.status).toBe(404);
		expect(asObject(wrongTask.body.error).code).toBe('not_found');
		expect(wrongTask.body).not.toHaveProperty('result');
		expect((await taskReply()).revision).toBe(String(revision));
	});

	const lookupPath = observed.lookups.find((path) => path.includes(originalOperation));
	if (!lookupPath) throw new Error('Actual original receipt lookup was not observed');
	const requiredPath = (predicate: (url: URL) => boolean) => {
		const path = observed.readPaths.find((path) => predicate(new URL(path, f.origin)));
		if (!path) throw new Error('Actual protected task read was not observed');
		return path;
	};
	const contentPaths = [
		...new Set(
			observed.readPaths.filter((path) => {
				const url = new URL(path, f.origin);
				return (
					url.pathname === taskBase(f) + '/content' &&
					url.searchParams.get('revision') === String(revision)
				);
			})
		)
	];
	for (const kind of ['snapshot', 'event'])
		expect(
			contentPaths.some((path) => new URL(path, f.origin).searchParams.get('kind') === kind)
		).toBe(true);
	const protectedPaths = [
		requiredPath((url) => url.pathname === taskBase(f)),
		requiredPath((url) => url.pathname === taskBase(f) + '/history'),
		requiredPath((url) => url.pathname === taskBase(f) + '/assignment-targets'),
		...contentPaths
	];
	const assertNoProtectedResult = (reply: Reply) => {
		const containsProtectedResult = (value: unknown): boolean => {
			if (!value || typeof value !== 'object') return false;
			if (Array.isArray(value)) return value.some(containsProtectedResult);
			return Object.entries(value).some(
				([key, item]) =>
					['result', 'current', 'hex', 'public_snapshot'].includes(key) ||
					containsProtectedResult(item)
			);
		};
		// Boolean failures cannot echo an accidentally disclosed private response.
		expect(containsProtectedResult(reply.body)).toBe(false);
		const serialized = JSON.stringify(reply.body);
		expect(serialized.includes(f.largeBodyTail)).toBe(false);
		expect(serialized.includes('future public activity payload')).toBe(false);
	};
	const assertDenied = (reply: Reply, status: 401 | 403 | 404) => {
		expect(reply.status).toBe(status);
		expect(asObject(reply.body.error).code).toBe(status === 404 ? 'not_found' : 'not_authorized');
		assertNoProtectedResult(reply);
	};
	const sessionView = async (view: Page, identity: SessionIdentity) => {
		const reply = await realJson(view, '/api/v1/session', identity.proof);
		expect(reply.status).toBe(200);
		const session = asObject(reply.body.result);
		expect(asObject(session.account).id).toBe(identity.accountId);
		expect(session.instance_id).toBe(identity.instanceId);
		return session;
	};
	const positiveReads = async (view: Page, proof: string) => {
		const lookup = await realJson(view, lookupPath, proof);
		expect(lookup.status).toBe(200);
		expect(asObject(lookup.body.result).operation_id).toBe(originalOperation);
		expect(asObject(lookup.body.result).result).toEqual(originalCompletion!);
		for (const path of protectedPaths) {
			const reply = await realJson(view, path, proof);
			expect(reply.status).toBe(200);
			if (new URL(path, f.origin).pathname === taskBase(f))
				expect(asObject(asObject(reply.body.result).task).revision).toBe(String(revision));
		}
	};
	const foreignContext = await browser.newContext({
		viewport: { width: 1280, height: 900 },
		reducedMotion: 'reduce'
	});
	try {
		const foreignPage = await foreignContext.newPage();
		const foreignObserved = monitor(foreignPage, f);
		await watchCsp(foreignPage);
		let foreignIdentity!: SessionIdentity;
		await test.step('a genuine linked non-owner cannot read the original receipt, task or exact content', async () => {
			await positiveReads(page, observed.proof);
			await foreignPage.goto(f.origin + '/tasks');
			foreignIdentity = await signIn(foreignPage, f.foreignAccount);
			expect(foreignIdentity.accountId === ownerIdentity.accountId).toBe(false);
			expect(foreignIdentity.instanceId).toBe(ownerIdentity.instanceId);
			const session = await sessionView(foreignPage, foreignIdentity);
			expect(Array.isArray(session.organizations)).toBe(true);
			const link = (session.organizations as unknown[])
				.map(asObject)
				.find((link) => link.org_id === f.organizationId);
			if (!link)
				throw new Error('Actual foreign session must advertise its real organization link');
			expect(Array.isArray(link.office_ids)).toBe(true);
			expect((link.office_ids as unknown[]).includes(f.officeId)).toBe(true);
			foreignObserved.allowNetworkFailure(lookupPath);
			assertDenied(await realJson(foreignPage, lookupPath, foreignIdentity.proof), 404);
			for (const path of protectedPaths) {
				foreignObserved.allowNetworkFailure(path);
				assertDenied(await realJson(foreignPage, path, foreignIdentity.proof), 403);
			}
			await positiveReads(page, observed.proof);
			expect((await taskReply()).revision).toBe(String(revision));
			await assertNoCsp(foreignPage);
			expect(foreignObserved.errors).toEqual([]);
			await publicScreenshot(foreignPage, f, info, 'assignment-foreign-account.png');
		});
		await test.step('logout propagates to a live tab and revokes the saved actual cookie/proof pair', async () => {
			await close(page, f);
			const witness = await page.context().newPage();
			const witnessObserved = monitor(witness, f);
			await watchCsp(witness);
			// Session cookies affect the API path, so the root URL would filter them out.
			const cookies = await page.context().cookies(f.origin + '/api/v1/session');
			expect(cookies.length > 0).toBe(true);
			for (const cookie of cookies) rememberSecret(cookie.value);
			const proof = observed.proof;
			const lookup = observed.lookups.find((path) => path.includes(originalOperation));
			if (!lookup) throw new Error('Actual saved receipt lookup was not observed');
			try {
				await witness.goto(f.origin + '/tasks');
				await expect(witness.getByRole('button', { name: 'Sign out', exact: true })).toBeVisible();
				await queue(witness, f);
				await page.getByRole('button', { name: 'Sign out', exact: true }).click();
				for (const view of [page, witness]) {
					await expect(view.getByRole('button', { name: 'Sign in', exact: true })).toBeVisible();
					await expect(view.getByRole('heading', { name: 'Task queue', exact: true })).toHaveCount(
						0
					);
					await expect(view.getByRole('dialog')).toHaveCount(0);
				}
				const revoked = await browser.newContext();
				try {
					await revoked.addCookies(cookies);
					const stale = await revoked.newPage();
					await stale.goto(f.origin + '/tasks');
					for (const path of ['/api/v1/session', lookup]) {
						const denied = await realJson(stale, path, proof);
						expect(denied.status).toBe(401);
						expect(asObject(denied.body.error).code).toBe('not_authorized');
						expect(denied.body).not.toHaveProperty('result');
					}
					const denied = await realJson(stale, assignmentPath(f), proof, lostIntent!);
					expect(denied.status).toBe(401);
					expect(denied.body).not.toHaveProperty('result');
				} finally {
					await revoked.close();
				}
				await assertNoCsp(witness);
				expect(witnessObserved.errors).toEqual([]);
			} finally {
				await witness.close();
			}
			await page.reload();
			await expect(page.getByRole('button', { name: 'Sign in', exact: true })).toBeVisible();
			const accessibility = await new AxeBuilder({ page }).analyze();
			expect(accessibility.violations.map((violation) => violation.id)).toEqual([]);
			await publicScreenshot(page, f, info, 'assignment-signed-out.png');
		});
		await test.step('terminal registry revocation invalidates two independently signed-in owner sessions', async () => {
			const contexts: BrowserContext[] = [];
			try {
				const sessions: {
					label: string;
					context: BrowserContext;
					view: Page;
					watched: ReturnType<typeof monitor>;
					identity: SessionIdentity;
					cookies: Awaited<ReturnType<BrowserContext['cookies']>>;
				}[] = [];
				for (const label of ['A', 'B']) {
					const context = await browser.newContext({
						viewport: { width: 1280, height: 900 },
						reducedMotion: 'reduce'
					});
					contexts.push(context);
					const view = await context.newPage();
					const watched = monitor(view, f);
					await watchCsp(view);
					await view.goto(f.origin + '/tasks');
					const identity = await signIn(view, f);
					expect(identity.accountId).toBe(ownerIdentity.accountId);
					expect(identity.instanceId).toBe(ownerIdentity.instanceId);
					await sessionView(view, identity);
					await queue(view, f);
					await open(view, f, revision);
					await assertPair(view, f, current);
					await positiveReads(view, identity.proof);
					const cookies = await context.cookies(f.origin + '/api/v1/session');
					expect(cookies.length > 0).toBe(true);
					for (const cookie of cookies) rememberSecret(cookie.value);
					sessions.push({ label, context, view, watched, identity, cookies });
				}
				const [a, b] = sessions;
				expect(a.identity.proof === b.identity.proof).toBe(false);
				expect(
					a.cookies.some((left) =>
						b.cookies.some(
							(right) =>
								left.name === right.name &&
								left.domain === right.domain &&
								left.path === right.path &&
								left.value === right.value
						)
					)
				).toBe(false);
				// This read-only control reuses the accepted timestamp, never its replay ID.
				const alphabet = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';
				const unknownOperation =
					originalOperation.slice(0, 10) +
					[...randomBytes(16)].map((byte) => alphabet[byte & 31]).join('');
				expect(unknownOperation).toMatch(/^[0-7][0-9A-HJKMNP-TV-Z]{25}$/);
				const submittedOperationIds = () =>
					new Set([
						...peerOperationIds,
						...observed.intents.map((intent) => intent.operation_id),
						...foreignObserved.intents.map((intent) => intent.operation_id),
						...sessions.flatMap(({ watched }) =>
							watched.intents.map((intent) => intent.operation_id)
						)
					]);
				expect(submittedOperationIds().has(unknownOperation)).toBe(false);
				const knownLookup = new URL(lookupPath, f.origin);
				expect(knownLookup.pathname.endsWith('/' + originalOperation)).toBe(true);
				const unknownLookup = new URL(knownLookup);
				unknownLookup.pathname =
					knownLookup.pathname.slice(0, -originalOperation.length) + unknownOperation;
				expect(unknownLookup.search).toBe(knownLookup.search);
				const unknownPath = unknownLookup.pathname + unknownLookup.search;
				a.watched.allowNetworkFailure(unknownPath);
				assertDenied(await realJson(a.view, unknownPath, a.identity.proof), 404);
				await sessionView(foreignPage, foreignIdentity);
				await revokeOwnerAccount(f.accountRevocation, ownerIdentity.accountId);
				for (const { label, context, view, watched, identity, cookies } of sessions) {
					for (const path of ['/api/v1/session', lookupPath, ...protectedPaths]) {
						watched.allowNetworkFailure(path);
						// Restore only that session's saved pair before every request, even
						// if an earlier 401 response cleared its browser cookie.
						await context.addCookies(cookies);
						assertDenied(await realJson(view, path, identity.proof), 401);
					}
					watched.allowNetworkFailure(assignmentPath(f));
					await context.addCookies(cookies);
					assertDenied(await realJson(view, assignmentPath(f), identity.proof, lostIntent!), 401);
					const submitted = watched.intents.length;
					await context.addCookies(cookies);
					await view.reload();
					await expect(view.getByRole('button', { name: 'Sign in', exact: true })).toBeVisible();
					await expect(view.getByRole('heading', { name: 'Task queue', exact: true })).toHaveCount(
						0
					);
					await expect(view.getByRole('dialog')).toHaveCount(0);
					expect(watched.intents.length).toBe(submitted);
					const accessibility = await new AxeBuilder({ page: view }).analyze();
					expect(accessibility.violations.map((violation) => violation.id)).toEqual([]);
					await assertNoCsp(view);
					expect(watched.errors).toEqual([]);
					await publicScreenshot(view, f, info, `assignment-account-revoked-${label}.png`);
				}
				const version = await realJson(foreignPage, '/api/v1/version');
				expect(version.status).toBe(200);
				await sessionView(foreignPage, foreignIdentity);
				foreignObserved.allowUnavailableLookups([lookupPath, unknownPath]);
				const unavailable = [];
				for (const path of [lookupPath, unknownPath]) {
					const reply = await realJson(foreignPage, path, foreignIdentity.proof);
					expect(reply.status).toBe(503);
					expect(asObject(reply.body.error).code).toBe('unavailable');
					assertNoProtectedResult(reply);
					unavailable.push(reply.body);
				}
				expect(isDeepStrictEqual(unavailable[0], unavailable[1])).toBe(true);
				expect(submittedOperationIds().has(unknownOperation)).toBe(false);
				await assertNoCsp(foreignPage);
				expect(foreignObserved.errors).toEqual([]);
			} finally {
				await Promise.all(contexts.map((context) => context.close()));
			}
		});
		await assertNoCsp(page);
		expect(observed.errors).toEqual([]);
	} finally {
		await foreignContext.close();
	}
});
