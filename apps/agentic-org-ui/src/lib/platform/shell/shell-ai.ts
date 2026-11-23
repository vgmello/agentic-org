import { createContext } from 'svelte';

export interface ShellAiAskOptions {
	decisionId?: string;
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
