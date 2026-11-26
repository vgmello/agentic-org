import { getContext, setContext } from 'svelte';

export interface ShellCommandHost {
	openPalette: (prefill?: string) => void;
	toggleTess: () => void;
	toggleTessExpand?: () => void;
	openTessChat: () => Promise<void>;
	openShortcuts?: () => void;
	openNewTask?: (opts?: { projectId?: string | null; agentId?: string | null }) => void;
}

const SHELL_COMMAND_KEY = Symbol('agentic-org.shell-command');

export function provideShellCommand(host: ShellCommandHost): ShellCommandHost {
	return setContext(SHELL_COMMAND_KEY, host);
}

export function useShellCommand(): ShellCommandHost | undefined {
	return getContext<ShellCommandHost | undefined>(SHELL_COMMAND_KEY);
}
