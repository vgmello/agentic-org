import { toAppError, type OfficeApi } from '$lib/office/api';
import { validateName } from '$lib/name';
import type { AppError, Connection } from '$lib/proto';

/**
 * Where the Create organization flow is. `editing` carries an error on the name field when the
 * owner can fix it by typing; `failed` is for everything else (the office, the disk), where the
 * only move is to try again.
 */
export type Phase =
	| { kind: 'editing'; nameError?: string }
	| { kind: 'starting' }
	| { kind: 'creating' }
	| { kind: 'failed'; error: AppError; during: 'starting' | 'creating' }
	| { kind: 'done'; connection: Connection };

/** True while the form must not accept input: working, or already done and navigating away. */
export const isBusy = (p: Phase) => p.kind === 'starting' || p.kind === 'creating' || p.kind === 'done';

/** Errors that are about what the owner typed, so the form stays open with the message by the field. */
const FIXABLE = new Set(['invalid_name', 'already_exists']);

/**
 * Run the flow from the name to a remembered connection, reporting each phase as it begins.
 * Every failure ends in a phase; this never throws.
 */
export async function createOrganization(
	api: OfficeApi,
	rawName: string,
	report: (phase: Phase) => void
): Promise<void> {
	const checked = validateName(rawName);
	if (!checked.ok) return report({ kind: 'editing', nameError: checked.message });

	report({ kind: 'starting' });
	try {
		await api.ensureRunning();
	} catch (e) {
		return report({ kind: 'failed', error: toAppError(e), during: 'starting' });
	}

	report({ kind: 'creating' });
	try {
		report({ kind: 'done', connection: await api.createOrganization(checked.name) });
	} catch (e) {
		const error = toAppError(e);
		if (error.source === 'api' && FIXABLE.has(error.code))
			report({ kind: 'editing', nameError: error.message });
		else report({ kind: 'failed', error, during: 'creating' });
	}
}

/** The headline and advice for a failure, in the owner's words. */
export function describeFailure(error: AppError): { title: string; hint: string } {
	switch (error.code) {
		case 'not_installed':
			return { title: "The office isn't installed", hint: 'agentic.org needs to install its office server on this Mac.' };
		case 'needs_approval':
			return { title: 'Approval needed', hint: 'Open System Settings → General → Login Items, allow agentic.org, then try again.' };
		case 'unreachable':
			return { title: "The office isn't answering", hint: 'It may still be starting. Try again in a moment.' };
		case 'storage_error':
			return { title: "Couldn't create the organization", hint: 'The office could not write to its storage. Check free disk space, then try again.' };
		default:
			return { title: 'Something went wrong', hint: error.message };
	}
}

/**
 * One run of the flow at a time. `submit` is ignored while a run is in progress, even if called
 * twice in the same tick (before the page has re-rendered its disabled button).
 */
export function createFlow(api: OfficeApi, report: (phase: Phase) => void) {
	let running = false;
	return {
		async submit(rawName: string): Promise<void> {
			if (running) return;
			running = true;
			try {
				await createOrganization(api, rawName, report);
			} finally {
				running = false;
			}
		}
	};
}
