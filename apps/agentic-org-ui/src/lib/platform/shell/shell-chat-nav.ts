import type { OrgSnapshot } from '$lib/proto';

function isTessMember(m: OrgSnapshot['members'][number]): boolean {
	return (
		m.name.toLowerCase().includes('tess') ||
		m.agent?.messaging_name?.toLowerCase() === 'tess' ||
		Boolean(m.agent?.role?.toLowerCase().includes('chief of staff'))
	);
}

/** Resolve Tess's active DM conversation ID lazily without eagerly bundling messaging code. */
export async function findTessDmId(
	organizationId: string,
	snapshot?: OrgSnapshot
): Promise<string | undefined> {
	try {
		const { chatApi } = await import('$lib/modules/messaging/api');
		const snap = await chatApi.snapshot(organizationId);
		const tessMember = (snapshot?.members ?? []).find((m) => isTessMember(m));
		const tessDm = snap.state.conversations.find(
			(c) =>
				c.kind === 'dm' &&
				!c.archived &&
				(Boolean(tessMember && c.members.includes(tessMember.id)) ||
					Boolean(c.name?.toLowerCase().includes('tess')))
		);
		return tessDm?.id;
	} catch {
		return undefined;
	}
}

/** Find or open a 1:1 DM conversation with a member, returning its conversation ID if available. */
export async function resolveMemberDmId(
	organizationId: string,
	memberId: string
): Promise<string | undefined> {
	try {
		const { chatApi, sender } = await import('$lib/modules/messaging/api');
		const snap = await chatApi.snapshot(organizationId);
		const existing = snap.state.conversations.find(
			(c) => c.kind === 'dm' && !c.archived && c.members.includes(memberId) && c.members.length <= 2
		);
		if (existing) return existing.id;
		const accepted = await sender(organizationId)({ method: 'dm.open', members: [memberId] });
		return accepted.conversation_id;
	} catch {
		return undefined;
	}
}
