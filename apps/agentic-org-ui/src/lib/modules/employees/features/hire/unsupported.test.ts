import { render } from 'svelte/server';
import { expect, test } from 'vitest';
import { Resource } from '$lib/components/resource.svelte';
import type {
	HireAccess,
	HireChoices,
	HirePreview,
	HireProposal,
	HireRequest,
	HireTeam
} from '../../types';
import HireFlow from './HireFlow.svelte';
import ReviewStep from './ReviewStep.svelte';

async function unsupportedResource<T>(message: string): Promise<Resource<T>> {
	const resource = new Resource<T>(async () => {
		throw { source: 'api', code: 'unsupported', message };
	});
	await resource.reload();
	return resource;
}

const proposal: HireProposal = {
	name: 'Nora',
	handle: 'nora',
	homeTeamId: 'team-platform',
	executionProfileId: 'prof-claude',
	model: 'claude-sonnet-4-6',
	officeId: 'hq'
};

const choices: HireChoices = {
	roles: [],
	people: [],
	takenHandles: [],
	profiles: [
		{
			id: 'prof-claude',
			name: 'Claude Code',
			driver: 'Claude Code',
			models: ['claude-sonnet-4-6'],
			readiness: { hq: 'ready' }
		}
	],
	offices: [{ id: 'hq', name: 'HQ', instance: 'hq.local' }]
};

const teams: HireTeam[] = [
	{ id: 'team-platform', name: 'Platform', ref: 'T-1', members: 1, maxMembers: 6 }
];

test('ReviewStep renders unsupported without Retry or Hire button and keeps Back', async () => {
	const previewResource = await unsupportedResource<HirePreview>(
		'This office release does not support hire dry runs.'
	);
	const { body } = render(ReviewStep, {
		props: {
			organizationId: 'org-1',
			organizationName: 'Acme',
			proposal,
			choices,
			teams,
			onback: () => {},
			onconfirm: async () => {},
			previewResource
		}
	});
	expect(body).toContain('Not available in this version');
	expect(body).toContain('This office release does not support hire dry runs.');
	expect(body).toContain('Back');
	expect(body).not.toContain('Retry');
	expect(body).not.toContain('Hire Nora');
	expect(body).not.toContain('Hire anyway');
});

test('HireFlow renders unsupported resume without Retry and keeps Start a new hire', async () => {
	const accessResource = new Resource<HireAccess>(async () => ({
		teams,
		seesAll: true,
		hr: { state: 'can_run' }
	}));
	await accessResource.reload();
	const resumeResource = await unsupportedResource<HireRequest | undefined>(
		'This office release does not support resuming hire requests.'
	);
	const { body } = render(HireFlow, {
		props: {
			organizationId: 'org-1',
			organizationName: 'Acme',
			requestId: 'req-1',
			accessResource,
			resumeResource
		}
	});
	expect(body).toContain('Not available in this version');
	expect(body).toContain('This office release does not support resuming hire requests.');
	expect(body).toContain('Start a new hire');
	expect(body).not.toContain('Retry');
});
