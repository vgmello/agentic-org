import { agentStatus, runtimeLabel } from '$lib/org/agents';
import { byName, isFormer, owner } from '$lib/org/model';
import type { Member, OrgSnapshot } from '$lib/proto';

/** Who a task can be assigned to: the owner first as "Name (you)", then agents and people by name, each with its kind in words (TSK-5). */
export function assigneeOptions(s: OrgSnapshot): { member: Member; label: string }[] {
	const boss = owner(s);
	const others = s.members.filter((m) => !m.owner && !isFormer(m)).sort(byName);
	return [...(boss ? [boss] : []), ...others].map((member) => ({
		member,
		label: member.owner ? `${member.name} (you)` : member.kind === 'agent' ? `${member.name} (agent · ${runtimeLabel(member.agent?.runtime)} · ${agentStatus(member.agent).word})` : `${member.name} (person)`
	}));
}

/** What to say before a task is given to someone who will not pick it up now; nothing when it is fine (TSK-5). */
export function assignWarning(s: OrgSnapshot, id: string | undefined): string | undefined {
	const m = s.members.find((x) => x.id === id);
	if (m?.kind === 'agent' && m.agent?.status === 'paused') return `${m.name} is paused. The task waits until you resume them.`;
	return undefined;
}

export const nameOf = (s: OrgSnapshot, id: string | undefined): string => (id ? (s.members.find((m) => m.id === id)?.name ?? 'Someone who left') : 'Unassigned');
