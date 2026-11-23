import { createContext } from 'svelte';

export interface ShellAiController {
	ask: (prompt: string) => void;
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
