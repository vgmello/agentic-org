import { createContext } from 'svelte';

export interface ShellAiDecisionContext {
	id: string;
	title: string;
	recommendation: string;
	risk: { label: string; sub: string };
	impact: { label: string; sub: string };
	backgroundContext: string;
	detailHref: string;
	projectId?: string;
	projectName: string;
}

export interface ShellAiAskOptions {
	decisionId?: string;
	decision?: ShellAiDecisionContext;
}

export interface ShellAiController {
	ask: (prompt: string, opts?: ShellAiAskOptions) => void;
}

const [getShellAiContext, setShellAiContext] = createContext<ShellAiController>();

export function provideShellAi(controller: ShellAiController): ShellAiController {
	return setShellAiContext(controller);
}

export function useShellAi(): ShellAiController | undefined {
	try {
		return getShellAiContext();
	} catch {
		return undefined;
	}
}
