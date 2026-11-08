import { byName, isFormer, owner } from '$lib/org/model';
import type { Member, OrgSnapshot } from '$lib/proto';

/** Who a task can be assigned to: the owner first as "Name (you)", then agents and people by name, each with its kind in words (TSK-5). */
export function assigneeOptions(s: OrgSnapshot): { member: Member; label: string }[] {
	const boss = owner(s);
	const others = s.members.filter((m) => !m.owner && !isFormer(m)).sort(byName);
	return [...(boss ? [boss] : []), ...others].map((member) => ({
		member,
		label: member.owner ? `${member.name} (you)` : `${member.name} (${member.kind === 'agent' ? 'agent' : 'person'})`
	}));
}

export const nameOf = (s: OrgSnapshot, id: string | undefined): string => (id ? (s.members.find((m) => m.id === id)?.name ?? 'Someone who left') : 'Unassigned');
