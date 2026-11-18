/**
 * The Create organization flow (offices-and-teams-ux.md §5a, OFC-15 to OFC-24), independent
 * of the page that shows it. One operation ID stands for one intent: it is sent again only
 * with the same payload, a lost reply is resolved by looking that ID up, and a new ID is
 * minted only after a confirmed refusal or when the person changes what is being created.
 */
import { trimWhiteSpace, validateName } from '$lib/name';
import type { OrganizationsApi, CreateParams, CreateReply } from './api';
import type { CreateField, CreateRefusal, OrganizationView } from './readers';
import { validSlug } from './slug';

export interface Fields {
	name: string;
	slug: string;
	repository: string;
	stateBranch: string;
}
export type FieldErrors = Partial<Record<CreateField, string>>;
/** What the failure alert offers. `none` is a refusal that trying again cannot change. */
export type FailureAction = 'try_again' | 'check_again' | 'open_list' | 'none';
export type Phase =
	| { kind: 'editing'; errors: FieldErrors }
	| { kind: 'creating'; name: string }
	/** The reply was lost: the same attempt is being looked up. `still` once HQ says it is in progress. */
	| { kind: 'checking'; name: string; still: boolean }
	| {
			kind: 'failed';
			name: string;
			code: string;
			message: string;
			action: FailureAction;
			operationId: string;
			at: Date;
	  }
	| { kind: 'created'; organization: OrganizationView };

/** True while the fields are read-only and Create sends nothing. */
/**
 * Why a phase is reported. `outcome` follows Create, Try again, Check again or HQ's answer;
 * `typed` follows a keystroke, which must never move focus away from the field being typed in.
 */
export type ReportCause = 'outcome' | 'typed';

export const isWorking = (phase: Phase): boolean =>
	phase.kind === 'creating' || phase.kind === 'checking';

export const NOT_ADMINISTRATOR = "Only this HQ's administrator can create organizations.";
const taken = (name: string) =>
	`There's already an organization called ${name} on this HQ. Choose another name.`;
const notCreated = (name: string) => `${name} wasn't created. Try again.`;

/** The inline message for a field the server refused, from its machine fields only. */
export function fieldMessage(
	field: CreateField,
	error: CreateRefusal,
	params: CreateParams
): string {
	const exists = error.code === 'already_exists';
	switch (field) {
		case 'name':
			return exists ? taken(params.name) : "HQ didn't accept this name. Change it and try again.";
		case 'slug':
			return exists
				? `There's already an organization with the slug ${params.slug} on this HQ. Choose another slug.`
				: 'Use lowercase letters, numbers and single hyphens, for example side-project.';
		case 'organization_repository':
			return error.rule === 'remote_required'
				? "Org Git must be a remote repository: an https:// or ssh:// address. A folder on a machine can't be used."
				: "HQ didn't accept this repository address. Check it and try again.";
		case 'state_branch':
			return 'Enter a branch name, for example main.';
	}
}

/** Checks before anything is sent. The server remains the authority on every field. */
export function checkFields(
	fields: Fields
): { ok: true; params: CreateParams } | { ok: false; errors: FieldErrors } {
	const errors: FieldErrors = {};
	const name = validateName(fields.name);
	if (!name.ok) errors.name = name.message;
	const slug = trimWhiteSpace(fields.slug);
	if (slug === '') errors.slug = 'Give the organization a slug.';
	else if (!validSlug(slug))
		errors.slug = 'Use lowercase letters, numbers and single hyphens, for example side-project.';
	const repository = trimWhiteSpace(fields.repository);
	if (repository === '') errors.organization_repository = "Enter the remote repository's address.";
	else if (!/^(?:https|ssh):\/\//.test(repository))
		errors.organization_repository =
			"Org Git must be a remote repository: an https:// or ssh:// address. A folder on a machine can't be used.";
	const stateBranch = trimWhiteSpace(fields.stateBranch);
	if (stateBranch === '') errors.state_branch = 'Enter the state branch.';
	if (!name.ok || Object.keys(errors).length > 0) return { ok: false, errors };
	return { ok: true, params: { name: name.name, slug, repository, stateBranch } };
}

/** What Copy details puts on the clipboard: never a key, token, proof or recovery code. */
export function failureDetails(
	failure: Extract<Phase, { kind: 'failed' }>,
	appVersion: string | undefined
): string {
	return [
		`Organization: ${failure.name}`,
		`Attempt: ${failure.operationId}`,
		`Error: ${failure.code}`,
		`Time: ${failure.at.toISOString()}`,
		`Version: ${appVersion ?? 'unknown'}`
	].join('\n');
}

export function createOrganizationFlow({
	api,
	report,
	sleep,
	now = () => new Date()
}: {
	api: OrganizationsApi;
	report: (phase: Phase, cause: ReportCause) => void;
	sleep: (ms: number, signal?: AbortSignal) => Promise<void>;
	now?: () => Date;
}) {
	/** The intent an operation ID stands for; kept while its outcome may still be "created". */
	let attempt: { id: string; payload: string; params: CreateParams } | undefined;
	let running = false;
	let phase: Phase = { kind: 'editing', errors: {} };
	const stopper = new AbortController();
	const show = (next: Phase, cause: ReportCause = 'outcome') => {
		phase = next;
		if (!stopper.signal.aborted) report(next, cause);
	};
	const fail = (code: string, message: string, action: FailureAction) => {
		const { id, params } = attempt!;
		show({ kind: 'failed', name: params.name, code, message, action, operationId: id, at: now() });
	};

	/** A confirmed refusal: nothing was stored, so the next attempt is a new one. */
	function refused(error: CreateRefusal) {
		const { params } = attempt!;
		const { name } = params;
		if (error.code === 'not_authorized') fail(error.code, NOT_ADMINISTRATOR, 'none');
		else if (error.field)
			show({
				kind: 'editing',
				errors: { [error.field]: fieldMessage(error.field, error, params) }
			});
		else if (error.code === 'already_exists')
			show({ kind: 'editing', errors: { name: taken(name) } });
		else if (error.code === 'invalid_name')
			show({
				kind: 'editing',
				errors: { name: "HQ didn't accept this name. Change it and try again." }
			});
		else if (error.code === 'storage_error')
			fail(
				error.code,
				`${name} wasn't created: HQ couldn't write its data on the HQ machine. Run \`agentic-org doctor\` there, then try again.`,
				'try_again'
			);
		else if (error.code === 'not_ready')
			fail(error.code, 'HQ is still starting. Try again in a moment.', 'try_again');
		else if (error.code === 'unavailable')
			fail(
				error.code,
				'Several organizations are being created right now. Try again in a moment.',
				'try_again'
			);
		else fail(error.code, notCreated(name), 'try_again');
		attempt = undefined;
	}

	/** Reads the attempt's state until it is known; never sends the create again. */
	async function follow() {
		const { id, params } = attempt!;
		const { name } = params;
		const result = await api.follow(id, {
			sleep,
			signal: stopper.signal,
			waiting: (reply) => {
				if (phase.kind === 'checking' && reply === 'accepted' && !phase.still)
					show({ kind: 'checking', name, still: true });
			}
		});
		switch (result.kind) {
			case 'stopped':
				return;
			case 'completed':
				attempt = undefined;
				return show({ kind: 'created', organization: result.organization });
			case 'rejected':
				return refused(result.error);
			// HQ never stored it: the same attempt may be sent again.
			case 'missing':
				return fail('not_found', notCreated(name), 'try_again');
			case 'unreachable':
				return fail(
					'unreachable',
					`Couldn't confirm whether ${name} was created, because HQ can't be reached.`,
					'check_again'
				);
			case 'session':
				return fail(
					'session_ended',
					`Sign in again, then check whether ${name} was created.`,
					'check_again'
				);
			case 'expired':
				fail(
					'expired',
					`This attempt is too old to check. Look for ${name} in the organization list before creating it again.`,
					'open_list'
				);
				attempt = undefined;
		}
	}

	async function answered(reply: CreateReply) {
		const { name } = attempt!.params;
		switch (reply.kind) {
			case 'accepted':
				return follow();
			case 'completed':
				attempt = undefined;
				return show({ kind: 'created', organization: reply.organization });
			case 'rejected':
				return refused(reply.error);
			case 'unknown':
				show({ kind: 'checking', name, still: false });
				return follow();
			case 'unavailable':
				return fail(
					reply.code,
					reply.code === 'not_ready'
						? 'HQ is still starting. Try again in a moment.'
						: `HQ couldn't be asked to create ${name}. Try again in a moment.`,
					'try_again'
				);
			case 'expired':
				fail(
					'expired',
					`This attempt is too old to check. Look for ${name} in the organization list before creating it again.`,
					'open_list'
				);
				attempt = undefined;
				return;
			// The sign-in-again dialog is showing; the form keeps its text and the attempt its ID.
			case 'session':
				return show({ kind: 'editing', errors: {} });
			case 'not_sent':
				return fail('not_sent', notCreated(name), 'try_again');
		}
	}

	async function once(work: () => Promise<void>) {
		if (running) return;
		running = true;
		try {
			await work();
		} finally {
			running = false;
		}
	}

	return {
		/** Create organization and Try again. Ignored while an attempt is in flight (OFC-18). */
		submit: (fields: Fields) =>
			once(async () => {
				const checked = checkFields(fields);
				if (!checked.ok) return show({ kind: 'editing', errors: checked.errors });
				const payload = JSON.stringify(checked.params);
				if (attempt?.payload !== payload) {
					let id: string;
					try {
						id = api.newOperationId();
					} catch {
						attempt = undefined;
						return show({
							kind: 'failed',
							name: checked.params.name,
							code: 'clock',
							message: notCreated(checked.params.name),
							action: 'try_again',
							operationId: '',
							at: now()
						});
					}
					attempt = { id, payload, params: checked.params };
				}
				show({ kind: 'creating', name: checked.params.name });
				// No abort signal: leaving the page must not cancel a create that is on its way (OFC-23).
				await answered(await api.create(attempt.id, checked.params));
			}),
		/** Check again: looks the same attempt up; sends nothing new. */
		checkAgain: () =>
			once(async () => {
				if (!attempt || phase.kind !== 'failed' || phase.action !== 'check_again') return;
				show({ kind: 'checking', name: attempt.params.name, still: false });
				await follow();
			}),
		/** Typing in a field clears that field's error. */
		edited(field: CreateField) {
			if (phase.kind !== 'editing' || !phase.errors[field]) return;
			const { [field]: _cleared, ...errors } = phase.errors;
			show({ kind: 'editing', errors }, 'typed');
		},
		/** The page is going away. The create itself continues at HQ (OFC-23). */
		stop: () => stopper.abort()
	};
}
