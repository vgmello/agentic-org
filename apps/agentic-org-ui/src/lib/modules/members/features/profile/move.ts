// The Move home team preview (our-org-in-the-app §8): what changes, in the spec's words, before anything is sent.
import type { OrgSnapshot } from '$lib/proto';

export function movePreview(
	s: Pick<OrgSnapshot, 'teams'>,
	member: { id: string; name: string },
	fromId: string,
	toId: string
): string[] {
	const name = (id: string) => s.teams.find((t) => t.id === id)?.name ?? 'an unknown team';
	const lines = [
		`${member.name} moves from ${name(fromId)} to ${name(toId)}. Current project and task assignments stay as recorded.`
	];
	if (s.teams.find((t) => t.id === fromId)?.lead_member_id === member.id)
		lines.push(`${member.name} leads ${name(fromId)} today; ${name(fromId)} will have no lead.`);
	return lines;
}

/**
 * Why a team can't be the destination, in words for its row, or undefined when it can: the current home team, or a
 * team the employee is already in (HQ refuses moving someone into a team they belong to).
 */
export function moveBlock(
	s: Pick<OrgSnapshot, 'members'>,
	member: { id: string; name: string },
	fromId: string,
	teamId: string
): string | undefined {
	if (teamId === fromId) return 'Current home team';
	if (s.members.some((m) => m.id === member.id && m.home_team_id === teamId))
		return `${member.name} is already in this team`;
	return undefined;
}
