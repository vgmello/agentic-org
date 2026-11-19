import { describe, expect, it } from 'vitest';
import type { ProfileChoice } from '../../types';
import { chooseProfile, initialForm, missingText, modelOptions, toEdit, unchanged } from './form';

const primary: ProfileChoice = {
	id: 'primary-pool',
	name: 'Primary pool',
	runtimes: ['Claude Code'],
	models: [
		{ id: 'claude-sonnet-5', supporting: 2, ready: 2, stale: false },
		{ id: 'claude-opus-5', supporting: 2, ready: 1, stale: false }
	]
};
const codex: ProfileChoice = {
	id: 'codex-pool',
	name: 'Codex pool',
	runtimes: ['Codex'],
	models: [{ id: 'gpt-5.5-codex', supporting: 1, ready: 0, stale: true }]
};
const exec = {
	profile: { id: 'primary-pool', name: 'Primary pool' },
	modelId: 'claude-sonnet-5',
	enabled: true,
	runtimes: ['Claude Code'],
	revision: 3,
	connections: [],
	tools: [],
	hasPrompt: true
};

describe('the execution form', () => {
	it('starts from the saved configuration, or empty without one', () => {
		expect(initialForm({ execution: exec })).toEqual({
			profileId: 'primary-pool',
			modelId: 'claude-sonnet-5'
		});
		expect(initialForm({ execution: null })).toEqual({ profileId: '', modelId: '' });
	});

	it('never picks a model for the person when the profile changes', () => {
		const form = { profileId: 'primary-pool', modelId: 'claude-sonnet-5' };
		expect(chooseProfile(form, codex)).toEqual({ profileId: 'codex-pool', modelId: '' });
		expect(chooseProfile({ profileId: 'codex-pool', modelId: 'claude-opus-5' }, primary)).toEqual({
			profileId: 'primary-pool',
			modelId: 'claude-opus-5'
		});
		expect(chooseProfile(form, undefined)).toEqual({ profileId: '', modelId: '' });
	});

	it('keeps a saved model the office does not report, marked as not offered', () => {
		expect(modelOptions(codex, 'gpt-4-codex')).toEqual([
			{ id: 'gpt-5.5-codex', offered: true },
			{ id: 'gpt-4-codex', offered: false }
		]);
		expect(modelOptions(primary, 'claude-opus-5')).toHaveLength(2);
		expect(modelOptions({ ...codex, models: null }, undefined)).toEqual([]);
	});

	it('names what is missing before Save', () => {
		expect(missingText({ profileId: '', modelId: '' })).toBe(
			'Choose a model profile and a model to save.'
		);
		expect(missingText({ profileId: 'p', modelId: ' ' })).toBe('Choose a model to save.');
		expect(missingText({ profileId: '', modelId: 'm' })).toBe('Choose a model profile to save.');
		expect(missingText({ profileId: 'p', modelId: 'm' })).toBeUndefined();
	});

	it('knows an unchanged form; a save never stands in for turning execution on', () => {
		const form = initialForm({ execution: exec });
		expect(unchanged(form, { execution: exec })).toBe(true);
		expect(unchanged(form, { execution: { ...exec, enabled: false } })).toBe(true);
		expect(unchanged({ ...form, modelId: 'claude-opus-5' }, { execution: exec })).toBe(false);
	});

	it('sends the revision the person saw, with a trimmed model', () => {
		expect(toEdit({ profileId: 'p', modelId: ' m ' }, 4)).toEqual({
			profileId: 'p',
			modelId: 'm',
			expectedRevision: 4
		});
	});
});
