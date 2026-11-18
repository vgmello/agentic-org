/**
 * Runtime readers for the organization routes of web API v1 (docs/design/web-api-v1.md):
 * the organization view, the list with its creates in flight, and the create's refusals.
 * Unknown fields are ignored; a missing required field refuses the whole payload.
 * The shared cases are crates/org-proto/test-vectors/v2/gateway-organizations.json.
 */
import { readOffice, readOperationId, WireDecodeError } from '../http/wire';

const refuse = (): never => {
	throw new WireDecodeError();
};
function record(value: unknown): Record<string, unknown> {
	if (!value || typeof value !== 'object' || Array.isArray(value)) return refuse();
	return value as Record<string, unknown>;
}
const own = (value: Record<string, unknown>, key: string): unknown =>
	Object.hasOwn(value, key) ? value[key] : undefined;
const text = (value: unknown): string => (typeof value === 'string' ? value : refuse());
const present = (value: unknown): string => text(value) || refuse();
const list = <T>(value: unknown, item: (value: unknown) => T): T[] =>
	Array.isArray(value) ? value.map(item) : refuse();
function guarded<T>(reader: (value: unknown) => T): (value: unknown) => T {
	return (value) => {
		try {
			return reader(value);
		} catch {
			throw new WireDecodeError();
		}
	};
}

const RECOVERY = ['pending', 'delivered', 'acknowledged'] as const;
export type Recovery = (typeof RECOVERY)[number] | 'unknown';
export type OfficeView = ReturnType<typeof readOffice>;
export interface OrganizationView {
	id: string;
	slug: string;
	name: string;
	created_at: string;
	/** The account's person in this organization. */
	employee_id: string;
	recovery: Recovery;
	/** The organization's offices on this instance, HQ first. */
	offices: OfficeView[];
}

export const readOrganizationView = guarded((value): OrganizationView => {
	const input = record(value);
	const recovery = text(own(input, 'recovery'));
	const offices = list(own(input, 'offices'), readOffice);
	return {
		id: present(own(input, 'id')),
		slug: present(own(input, 'slug')),
		name: present(own(input, 'name')),
		created_at: present(own(input, 'created_at')),
		employee_id: present(own(input, 'employee_id')),
		// The server lists HQ first; this keeps that true for a reply that does not (stable).
		offices: [...offices.filter((o) => o.role === 'hq'), ...offices.filter((o) => o.role !== 'hq')],
		recovery: (RECOVERY as readonly string[]).includes(recovery)
			? (recovery as Recovery)
			: 'unknown'
	};
});

/** A create of this account that has not completed; its operation is followed through the lookup. */
export interface CreatingView {
	operation_id: string;
	name: string;
	slug: string;
}
const readCreating = (value: unknown): CreatingView => {
	const input = record(value);
	return {
		operation_id: readOperationId(own(input, 'operation_id')),
		name: present(own(input, 'name')),
		slug: present(own(input, 'slug'))
	};
};
export interface OrganizationList {
	organizations: OrganizationView[];
	creating: CreatingView[];
}
export const readOrganizationsList = guarded((value): OrganizationList => {
	const input = record(value);
	const creating = own(input, 'creating');
	return {
		organizations: list(own(input, 'organizations'), readOrganizationView),
		// A server from before creates were listed sends none.
		creating: creating === undefined ? [] : list(creating, readCreating)
	};
});

export const CREATE_FIELDS = ['name', 'slug', 'organization_repository', 'state_branch'] as const;
export type CreateField = (typeof CREATE_FIELDS)[number];
/** Machine fields of a refusal only: the server's message text never reaches the page. */
export interface CreateRefusal {
	code: string;
	/** Present only when one of the form's inputs is at fault. */
	field?: CreateField;
	rule?: string;
}
export const readCreateRefusal = guarded((value): CreateRefusal => {
	const input = record(value);
	const field = own(input, 'field');
	const rule = own(input, 'rule');
	return {
		code: present(own(input, 'code')),
		...(typeof field === 'string' && (CREATE_FIELDS as readonly string[]).includes(field)
			? { field: field as CreateField }
			: {}),
		...(typeof rule === 'string' && rule ? { rule } : {})
	};
});

/** `{error}` bodies: the web process refused before any host call, or the host could not be asked. */
export const readErrorBody = guarded((value): CreateRefusal =>
	readCreateRefusal(own(record(value), 'error'))
);

export type OutcomeBody =
	| { outcome: 'accepted' | 'unknown' }
	| { outcome: 'completed'; organization: OrganizationView }
	| { outcome: 'rejected'; error: CreateRefusal };
/**
 * A create reply or a lookup result. The operation ID must be the one asked about: an
 * answer about another operation is refused, never shown as this one's outcome.
 */
export function readOutcomeBody(value: unknown, operationId: string): OutcomeBody {
	return guarded((value): OutcomeBody => {
		const input = record(value);
		if (readOperationId(own(input, 'operation_id')) !== operationId) return refuse();
		const outcome = text(own(input, 'outcome'));
		if (outcome === 'accepted' || outcome === 'unknown') return { outcome };
		if (outcome === 'completed')
			return { outcome, organization: readOrganizationView(own(input, 'result')) };
		if (outcome === 'rejected') return { outcome, error: readCreateRefusal(own(input, 'error')) };
		return refuse();
	})(value);
}

export interface InstanceName {
	instance_id: string;
	name: string;
	server_version: string;
	server_time_ms: number;
}
