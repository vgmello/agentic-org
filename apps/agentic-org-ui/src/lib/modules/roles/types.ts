// What the Roles screens read and change. Shapes follow the roles table in
// docs/design/organization-data/01-organization-structure.md (a shared `roles/<role-id>.md` record: stable `id`,
// optional organization-unique `key`, `schema_version` 1, `revision`, `name`, `body_markdown`, common creator and
// update attribution) and the employee's optional `role_id` (04-agent-structure.md). A role grants no authority.

/** Who wrote a record, as the common attribution columns say (`*_by_kind`, `*_by_id`, `*_at`). */
export interface Attribution {
	/** RFC 3339; missing on older records. */
	at?: string;
	byKind: 'employee' | 'system' | 'unknown';
	/** Set when `byKind` is `employee`. */
	byId?: string;
	/** The employee's name, resolved by the server, when it still resolves. */
	byName?: string;
}

/** A role file whose details the office could not read: "<role>: details not recognized (<field>)." */
export interface RoleProblem {
	/** The front-matter field that was not recognized: `key`, `name`, `schema_version`… */
	field: string;
}

/** Organization data could not be reached: what is shown is the last copy, from `copiedAt`. */
export interface StaleCopy {
	/** RFC 3339: when the last copy was taken. */
	copiedAt: string;
}

/** What the viewer may do here. The server authorizes every change regardless. */
export interface RolesViewer {
	canEdit: boolean;
}

/** Whether the HR lead can run, and so draft roles (our-org-in-the-app §2a). */
export interface HrLead {
	canRun: boolean;
}

export interface RoleSummary {
	id: string;
	key?: string;
	name: string;
	revision: number;
	/** Employees, agents and people, whose `role_id` is this role. */
	employeeCount: number;
	invalid?: RoleProblem;
}

export interface RolesOverview {
	roles: RoleSummary[];
	viewer: RolesViewer;
	hr: HrLead;
	stale?: StaleCopy;
}

export interface Role {
	id: string;
	key?: string;
	schemaVersion: number;
	revision: number;
	name: string;
	bodyMarkdown: string;
	created: Attribution;
	updated: Attribution;
	invalid?: RoleProblem;
	/** Development processes in use that name this role's key: the key can't be changed or removed (decision 7). */
	keyNamedBy?: string[];
}

/** An employee whose `role_id` references the role. */
export interface RoleEmployee {
	id: string;
	name: string;
	kind: 'agent' | 'person';
	title?: string;
}

/** One revision of the role, newest first in a history. */
export interface RoleRevision {
	revision: number;
	name: string;
	bodyMarkdown: string;
	updated: Attribution;
}

export interface RoleDetail {
	role: Role;
	employees: RoleEmployee[];
	/** Every revision, newest first; the first is the current one. */
	history: RoleRevision[];
	viewer: RolesViewer;
	stale?: StaleCopy;
}

/** A role's editable content: a new role, an HR lead's draft, or an edit. */
export interface RoleDraft {
	name: string;
	/** Empty or missing: no key. */
	key?: string;
	bodyMarkdown: string;
}

export interface RoleEdit extends RoleDraft {
	/** The revision the person looked at: a newer one is a conflict. */
	expectedRevision: number;
}

/** A refusal tied to one field of the editor (`already_exists` key, `invalid_name`…). */
export type RoleField = 'name' | 'key' | 'body';
