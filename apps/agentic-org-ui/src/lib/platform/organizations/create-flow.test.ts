import { describe, expect, it, vi } from 'vitest';
import { NotDispatchedError } from '../http/errors';
import {
	checkFields,
	createOrganizationFlow,
	failureDetails,
	isWorking,
	NOT_ADMINISTRATOR,
	type Phase
} from './create-flow';
import {
	accepted,
	completed,
	fields,
	looked,
	lookedAccepted,
	lookedCompleted,
	notFound,
	OP,
	OP2,
	organization,
	rejected,
	scripted,
	unknown
} from './testing';

function setup() {
	const s = scripted();
	const phases: Phase[] = [];
	const sleep = vi.fn(async (_ms: number, _signal?: AbortSignal) => {});
	const flow = createOrganizationFlow({
		api: s.api,
		report: (p) => phases.push(p),
		sleep,
		now: () => new Date('2026-11-18T10:00:00Z')
	});
	const sent = () =>
		s.submit.mock.calls.map(([, body]) => (body as { operation_id: string }).operation_id);
	const last = () => phases.at(-1)!;
	return { ...s, flow, phases, sleep, sent, last };
}
const failed = (phase: Phase) => {
	if (phase.kind !== 'failed') throw new Error(`expected failed, got ${phase.kind}`);
	return phase;
};
const down = { status: 503, body: { error: { code: 'unavailable', message: 'm' } } };

describe('checks before sending', () => {
	it('sends nothing for an invalid name and shows its inline message (OFC-16)', async () => {
		const s = setup();
		for (const [name, message] of [
			['  ', 'Give the organization a name.'],
			['x'.repeat(65), 'Use 64 characters or fewer.'],
			['a\u0007b', "The name can't contain control characters."],
			['a‮b', "The name can't contain invisible or direction-changing characters."]
		]) {
			await s.flow.submit({ ...fields, name });
			expect(s.last()).toEqual({ kind: 'editing', errors: { name: message } });
		}
		expect(s.submit).not.toHaveBeenCalled();
		expect(s.newId).not.toHaveBeenCalled();
	});
	it('requires every field and reports all of them at once', () => {
		const result = checkFields({ name: '', slug: '', repository: '', stateBranch: ' ' });
		expect(result).toEqual({
			ok: false,
			errors: {
				name: 'Give the organization a name.',
				slug: 'Give the organization a slug.',
				organization_repository: "Enter the remote repository's address.",
				state_branch: 'Enter the state branch.'
			}
		});
	});
	it('refuses a slug that is not kebab-case and a repository that is a folder', () => {
		const result = checkFields({ ...fields, slug: 'Side Project', repository: '/Users/alex/org' });
		expect(result.ok).toBe(false);
		expect(Object.keys((result as { errors: object }).errors)).toEqual([
			'slug',
			'organization_repository'
		]);
	});
	it('trims what it sends', () => {
		expect(
			checkFields({
				name: ' Side Project ',
				slug: ' side-project ',
				repository: ' ssh://git@example.test/o.git ',
				stateBranch: ' main '
			})
		).toEqual({
			ok: true,
			params: {
				name: 'Side Project',
				slug: 'side-project',
				repository: 'ssh://git@example.test/o.git',
				stateBranch: 'main'
			}
		});
	});
});

describe('one attempt', () => {
	it('a 200 completes without a lookup', async () => {
		const s = setup();
		s.submit.mockResolvedValueOnce(completed());
		await s.flow.submit(fields);
		expect(s.phases).toEqual([
			{ kind: 'creating', name: 'Side Project' },
			{ kind: 'created', organization }
		]);
		expect(s.read).not.toHaveBeenCalled();
	});
	it('a 202 is followed through the lookup of the same ID until it completes', async () => {
		const s = setup();
		s.submit.mockResolvedValueOnce(accepted());
		s.read.mockResolvedValueOnce(lookedAccepted()).mockResolvedValueOnce(lookedCompleted());
		await s.flow.submit(fields);
		expect(s.phases.map((p) => p.kind)).toEqual(['creating', 'created']);
		expect(s.sent()).toEqual([OP]);
		expect(s.read.mock.calls.every(([path]) => path.includes(`/operations/host/${OP}?`))).toBe(
			true
		);
		expect(s.sleep.mock.calls.map(([ms]) => ms)).toEqual([1000]);
	});
	it('a second submit while one is in flight sends nothing (OFC-18)', async () => {
		const s = setup();
		let reply!: (value: ReturnType<typeof completed>) => void;
		s.submit.mockReturnValueOnce(new Promise((resolve) => (reply = resolve)));
		const first = s.flow.submit(fields);
		await s.flow.submit(fields);
		await s.flow.submit({ ...fields, name: 'Another' });
		expect(isWorking(s.last())).toBe(true);
		reply(completed());
		await first;
		expect(s.submit).toHaveBeenCalledTimes(1);
		expect(s.newId).toHaveBeenCalledTimes(1);
	});
});

describe('a lost reply (OFC-21)', () => {
	it.each([
		['a 504', () => Promise.resolve(unknown())],
		['no reply', () => Promise.reject(new Error('network'))]
	])('%s shows Checking and looks the same attempt up, never sending again', async (_, reply) => {
		const s = setup();
		s.submit.mockImplementationOnce(reply);
		s.read.mockResolvedValueOnce(lookedAccepted()).mockResolvedValueOnce(lookedCompleted());
		await s.flow.submit(fields);
		expect(s.phases).toEqual([
			{ kind: 'creating', name: 'Side Project' },
			{ kind: 'checking', name: 'Side Project', still: false },
			{ kind: 'checking', name: 'Side Project', still: true },
			{ kind: 'created', organization }
		]);
		expect(s.sent()).toEqual([OP]);
	});
	it('no record offers Try again, which re-sends the SAME ID', async () => {
		const s = setup();
		s.submit.mockResolvedValueOnce(unknown()).mockResolvedValueOnce(completed());
		s.read.mockResolvedValueOnce(notFound);
		await s.flow.submit(fields);
		expect(failed(s.last())).toMatchObject({
			message: "Side Project wasn't created. Try again.",
			action: 'try_again',
			operationId: OP
		});
		await s.flow.submit(fields);
		expect(s.sent()).toEqual([OP, OP]);
		expect(s.last().kind).toBe('created');
	});
	it('an unreachable HQ offers Check again, which only looks up', async () => {
		const s = setup();
		s.submit.mockResolvedValueOnce(unknown());
		s.read
			.mockResolvedValueOnce(down)
			.mockResolvedValueOnce(down)
			.mockResolvedValueOnce(down)
			.mockResolvedValueOnce(lookedCompleted());
		await s.flow.submit(fields);
		expect(failed(s.last())).toMatchObject({
			message: "Couldn't confirm whether Side Project was created, because HQ can't be reached.",
			action: 'check_again',
			code: 'unreachable'
		});
		await s.flow.checkAgain();
		expect(s.last()).toEqual({ kind: 'created', organization });
		expect(s.sent()).toEqual([OP]);
	});
	it('an attempt too old to check points at the list', async () => {
		const s = setup();
		s.submit.mockResolvedValueOnce(unknown());
		s.read.mockResolvedValueOnce({
			status: 410,
			body: { error: { code: 'expired', message: 'm' } }
		});
		await s.flow.submit(fields);
		expect(failed(s.last())).toMatchObject({
			message:
				'This attempt is too old to check. Look for Side Project in the organization list before creating it again.',
			action: 'open_list'
		});
	});
	it('a stored failure found by the lookup shows its reason', async () => {
		const s = setup();
		s.submit.mockResolvedValueOnce(unknown());
		s.read.mockResolvedValueOnce(
			looked({ outcome: 'rejected', error: { code: 'storage_error', message: 'm' } })
		);
		await s.flow.submit(fields);
		expect(failed(s.last()).message).toBe(
			"Side Project wasn't created: HQ couldn't write its data on the HQ machine. Run `agentic-org doctor` there, then try again."
		);
	});
});

describe('refusals', () => {
	it('a rejected attempt keeps Try again, which is a NEW attempt (OFC-20)', async () => {
		const s = setup();
		s.submit
			.mockResolvedValueOnce(rejected(400, { code: 'clock_skew' }))
			.mockResolvedValueOnce(completed(OP2));
		await s.flow.submit(fields);
		expect(failed(s.last())).toMatchObject({
			message: "Side Project wasn't created. Try again.",
			action: 'try_again',
			code: 'clock_skew',
			operationId: OP
		});
		await s.flow.submit(fields);
		expect(s.sent()).toEqual([OP, OP2]);
		expect(s.last().kind).toBe('created');
	});
	it.each([
		[down, 'unavailable', "HQ couldn't be asked to create Side Project. Try again in a moment."],
		[
			{ status: 503, body: { error: { code: 'not_ready', message: 'm' } } },
			'not_ready',
			'HQ is still starting. Try again in a moment.'
		]
	])('a 503 Try again re-sends the SAME ID', async (reply, code, message) => {
		const s = setup();
		s.submit.mockResolvedValueOnce(reply).mockResolvedValueOnce(completed());
		await s.flow.submit(fields);
		expect(failed(s.last())).toMatchObject({ code, message, action: 'try_again' });
		await s.flow.submit(fields);
		expect(s.sent()).toEqual([OP, OP]);
	});
	it('a changed form after a 503 is a new intent and gets a new ID, never a reused one', async () => {
		const s = setup();
		s.submit.mockResolvedValueOnce(down).mockResolvedValueOnce(completed(OP2));
		await s.flow.submit(fields);
		await s.flow.submit({ ...fields, slug: 'another' });
		expect(s.sent()).toEqual([OP, OP2]);
	});
	it('a request that never left is retried with the same ID', async () => {
		const s = setup();
		s.submit.mockRejectedValueOnce(new NotDispatchedError()).mockResolvedValueOnce(completed());
		await s.flow.submit(fields);
		expect(failed(s.last()).action).toBe('try_again');
		await s.flow.submit(fields);
		expect(s.sent()).toEqual([OP, OP]);
	});
	it.each([
		[
			'name',
			'already_exists',
			undefined,
			"There's already an organization called Side Project on this HQ. Choose another name."
		],
		[
			'slug',
			'already_exists',
			undefined,
			"There's already an organization with the slug side-project on this HQ. Choose another slug."
		],
		[
			'slug',
			'invalid_request',
			'format',
			'Use lowercase letters, numbers and single hyphens, for example side-project.'
		],
		[
			'organization_repository',
			'invalid_request',
			'remote_required',
			"Org Git must be a remote repository: an https:// or ssh:// address. A folder on a machine can't be used."
		],
		['state_branch', 'invalid_request', 'format', 'Enter a branch name, for example main.']
	] as const)(
		'a refusal naming %s (%s) shows by that field and the next attempt is new',
		async (field, code, rule, message) => {
			const s = setup();
			const status = code === 'already_exists' ? 409 : 400;
			s.submit
				.mockResolvedValueOnce(
					code === 'already_exists'
						? rejected(status, { code, field })
						: { status, body: { error: { code, message: 'm', field, rule } } }
				)
				.mockResolvedValueOnce(completed(OP2));
			await s.flow.submit(fields);
			expect(s.last()).toEqual({ kind: 'editing', errors: { [field]: message } });
			s.flow.edited(field);
			expect(s.last()).toEqual({ kind: 'editing', errors: {} });
			await s.flow.submit(fields);
			expect(s.sent()).toEqual([OP, OP2]);
		}
	);
	it('already_exists without a field is the name (OFC-17)', async () => {
		const s = setup();
		s.submit.mockResolvedValueOnce(rejected(409, { code: 'already_exists' }));
		await s.flow.submit(fields);
		expect(s.last()).toEqual({
			kind: 'editing',
			errors: {
				name: "There's already an organization called Side Project on this HQ. Choose another name."
			}
		});
	});
	it('typing in one field keeps the other fields errors', async () => {
		const s = setup();
		await s.flow.submit({ ...fields, name: '', slug: '' });
		s.flow.edited('name');
		expect(s.last()).toEqual({
			kind: 'editing',
			errors: { slug: 'Give the organization a slug.' }
		});
		const reports = s.phases.length;
		s.flow.edited('name');
		expect(s.phases).toHaveLength(reports);
	});
	it('says which reports follow a keystroke, so the page moves focus only after a submit or a refusal', async () => {
		const s = scripted();
		const causes: string[] = [];
		const flow = createOrganizationFlow({
			api: s.api,
			report: (phase, cause) => causes.push(`${phase.kind}:${cause}`),
			sleep: async () => {}
		});
		await flow.submit({ ...fields, name: '', slug: '' });
		flow.edited('name');
		s.submit.mockResolvedValueOnce(rejected(409, { code: 'already_exists', field: 'slug' }));
		await flow.submit(fields);
		flow.edited('slug');
		expect(causes).toEqual([
			'editing:outcome',
			'editing:typed',
			'creating:outcome',
			'editing:outcome',
			'editing:typed'
		]);
	});
	it('not the administrator: the OFC-22 copy and no Try again', async () => {
		const s = setup();
		s.submit.mockResolvedValueOnce(rejected(403, { code: 'not_authorized' }));
		await s.flow.submit(fields);
		expect(failed(s.last())).toMatchObject({ message: NOT_ADMINISTRATOR, action: 'none' });
		expect(NOT_ADMINISTRATOR).toBe("Only this HQ's administrator can create organizations.");
	});
	it('an ended session returns to the form with its text; the same attempt is sent after signing in', async () => {
		const s = setup();
		s.submit.mockResolvedValueOnce({ status: 401, body: {} }).mockResolvedValueOnce(completed());
		await s.flow.submit(fields);
		expect(s.last()).toEqual({ kind: 'editing', errors: {} });
		await s.flow.submit(fields);
		expect(s.sent()).toEqual([OP, OP]);
	});
	it('a clock that cannot mint an ID fails without sending', async () => {
		const s = setup();
		s.newId.mockImplementationOnce(() => {
			throw new Error('not synchronized');
		});
		await s.flow.submit(fields);
		expect(failed(s.last())).toMatchObject({ code: 'clock', action: 'try_again' });
		expect(s.submit).not.toHaveBeenCalled();
	});
});

describe('leaving and details', () => {
	it('stopping ends the lookups and reports nothing more; it sends no cancel', async () => {
		const s = setup();
		s.submit.mockResolvedValueOnce(accepted());
		s.read.mockResolvedValue(lookedAccepted());
		s.sleep.mockImplementationOnce(async () => s.flow.stop());
		await s.flow.submit(fields);
		expect(s.phases).toEqual([{ kind: 'creating', name: 'Side Project' }]);
		expect(s.submit).toHaveBeenCalledTimes(1);
		expect(s.read).toHaveBeenCalledTimes(1);
	});
	it('Copy details holds the name, attempt, code, time and version, and nothing secret', async () => {
		const s = setup();
		s.submit.mockResolvedValueOnce(
			rejected(400, { code: 'storage_error', message: 'proof=abc token=def' })
		);
		await s.flow.submit(fields);
		const details = failureDetails(failed(s.last()), '1.2.3');
		expect(details).toBe(
			`Organization: Side Project\nAttempt: ${OP}\nError: storage_error\nTime: 2026-11-18T10:00:00.000Z\nVersion: 1.2.3`
		);
		expect(details).not.toMatch(/proof|token|recovery|key|https?:/i);
		expect(failureDetails(failed(s.last()), undefined)).toContain('Version: unknown');
	});
});
