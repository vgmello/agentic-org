// The Edit (execution) form's rules (agent-run-modes-ux §2; document 11 §4): a model profile and a canonical model,
// saved against the execution revision the person saw. No runtime or model is ever chosen for the person: a new
// profile keeps the model only when that profile offers it too.
import type { AgentSetupDetail, ExecutionEdit, ProfileChoice } from '../../types';

export interface ExecutionForm {
	profileId: string;
	modelId: string;
}

export const initialForm = (d: Pick<AgentSetupDetail, 'execution'>): ExecutionForm => ({
	profileId: d.execution?.profile.id ?? '',
	modelId: d.execution?.modelId ?? ''
});

/** The model offers for a profile, plus the saved model when the office doesn't report it (kept, never dropped). */
export function modelOptions(
	profile: ProfileChoice | undefined,
	saved: string | undefined
): { id: string; offered: boolean }[] {
	const listed = (profile?.models ?? []).map((m) => ({ id: m.id, offered: true }));
	if (saved && !listed.some((m) => m.id === saved)) listed.push({ id: saved, offered: false });
	return listed;
}

/** Choosing another profile: keep the model if the new profile offers it, otherwise ask for one. */
export function chooseProfile(
	form: ExecutionForm,
	profile: ProfileChoice | undefined
): ExecutionForm {
	const keeps = !!profile?.models?.some((m) => m.id === form.modelId);
	return { profileId: profile?.id ?? '', modelId: keeps ? form.modelId : '' };
}

/** What still blocks Save, in words; undefined when the form can be saved. */
export function missingText(form: ExecutionForm): string | undefined {
	if (!form.profileId && !form.modelId) return 'Choose a model profile and a model to save.';
	if (!form.profileId) return 'Choose a model profile to save.';
	if (!form.modelId.trim()) return 'Choose a model to save.';
	return undefined;
}

export const unchanged = (form: ExecutionForm, d: Pick<AgentSetupDetail, 'execution'>) =>
	!!d.execution &&
	form.profileId === d.execution.profile.id &&
	form.modelId === d.execution.modelId;

export const toEdit = (form: ExecutionForm, expectedRevision: number): ExecutionEdit => ({
	profileId: form.profileId,
	modelId: form.modelId.trim(),
	expectedRevision
});
