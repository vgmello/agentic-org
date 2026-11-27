import { describe, expect, it } from 'vitest';
import type { TessActionProposal } from './types';
import { isProtectedBuiltInProposal } from './tess-stream';

function proposal(
	kind: string,
	parameters: { label: string; value: string }[]
): TessActionProposal {
	return {
		id: 'prop-1',
		kind,
		title: 'Test proposal',
		summary: 'Summary',
		parameters,
		risk: 'high',
		ctaLabel: 'Apply',
		status: 'pending'
	};
}

describe('isProtectedBuiltInProposal', () => {
	it('blocks mutating proposals targeting HR, Tess, chief-of-staff, or tess-cos-N handles', () => {
		expect(
			isProtectedBuiltInProposal(proposal('member.offboard', [{ label: 'Agent', value: '@hr' }]))
		).toBe(true);
		expect(
			isProtectedBuiltInProposal(
				proposal('agent.pause', [{ label: 'Handle', value: 'tess-cos-2' }])
			)
		).toBe(true);
		expect(
			isProtectedBuiltInProposal(
				proposal('member.rename', [{ label: 'Target', value: 'Chief of Staff' }])
			)
		).toBe(true);
		expect(
			isProtectedBuiltInProposal(
				proposal('member.move', [{ label: 'Agent', value: 'chief-of-staff' }])
			)
		).toBe(true);
	});

	it('blocks proposals referencing a built-in agent by member ID when protectedMemberIds is supplied', () => {
		const protectedIds = ['019a0000-0000-7000-8000-000000000001'];
		expect(
			isProtectedBuiltInProposal(
				proposal('member.offboard', [
					{ label: 'Member ID', value: '019a0000-0000-7000-8000-000000000001' }
				]),
				protectedIds
			)
		).toBe(true);
	});

	it('allows non-built-in agents and non-member proposals', () => {
		expect(
			isProtectedBuiltInProposal(proposal('member.pause', [{ label: 'Agent', value: '@noor' }]))
		).toBe(false);
		expect(
			isProtectedBuiltInProposal(proposal('task.create', [{ label: 'Owner', value: 'tess' }]))
		).toBe(false);
	});
});
