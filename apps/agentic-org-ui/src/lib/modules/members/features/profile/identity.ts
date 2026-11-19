// Edit from the profile changes who an employee is, never how it runs (our-org-in-the-app §5, PROF-4): name, role,
// title, focus and handle for an agent; name and contact for a person. Runtime, model and the managed home are
// execution settings with their own revision, changed in Agent setup.
import type { AgentChange } from '$lib/org/actions';
import type { Member } from '$lib/proto';

export interface AgentIdentityForm {
	name: string;
	role: string;
	title: string;
	focus: string;
	handle: string;
}

export const agentIdentityForm = (m: Member): AgentIdentityForm => ({
	name: m.name,
	role: m.agent?.role ?? '',
	title: m.agent?.title ?? '',
	focus: m.agent?.focus ?? '',
	handle: m.agent?.messaging_name ?? ''
});

/** Only what differs from the record, and only identity fields: no runtime, model or home is ever sent. */
export function agentIdentityChange(m: Member, form: AgentIdentityForm): AgentChange {
	const a = m.agent;
	const change: AgentChange = {};
	if (form.name.trim() !== m.name) change.name = form.name;
	if (form.role.trim() !== (a?.role ?? '')) change.role = form.role;
	if (form.title.trim() !== (a?.title ?? '')) change.title = form.title;
	if (form.focus.trim() !== (a?.focus ?? '')) change.focus = form.focus;
	if (form.handle !== (a?.messaging_name ?? '')) change.messagingName = form.handle;
	return change;
}

/** The required identity fields: name, role and title (focus is optional; the handle has its own check). */
export const agentIdentityMissing = (form: AgentIdentityForm) =>
	form.name.trim() === '' || form.role.trim() === '' || form.title.trim() === '';

/**
 * The Offboard preview (our-org-in-the-app §9): the shared summary's lines about teams, tasks and reports, with the
 * retention line in the spec's words, naming the handle rather than "the name".
 */
export function offboardPreview(
	summary: string[],
	m: Pick<Member, 'name'>,
	handle: string
): string[] {
	const retention = `${m.name}'s record stays`;
	return [
		...summary.filter((line) => !line.startsWith(retention)),
		`${m.name}'s profile and home-team history stay in organization data. The handle @${handle} stays reserved: nobody else can use it.`
	];
}
