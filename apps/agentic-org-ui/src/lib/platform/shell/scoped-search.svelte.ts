// Reactive scoped-search registration for the shared ExecutiveCommandBar top header (Prototype Release 1149b4f1).
// Views that own a scoped work query (Team Work, Member Work, Project Work) register their live getter/setter
// so the shared header renders one ScopedSearchInput beside Workspace while each view owns its query & filtering.
import { getContext, setContext } from 'svelte';

export interface ScopedSearchBinding {
	getValue: () => string;
	setValue: (next: string) => void;
	label: string;
	placeholder: string;
	headingSelector?: string;
}

export interface ScopedSearchHost {
	readonly active: ScopedSearchBinding | undefined;
	register: (binding: ScopedSearchBinding) => () => void;
}

const SCOPED_SEARCH_KEY = Symbol('agentic-org.scoped-search');

export function createScopedSearchHost(): ScopedSearchHost {
	let activeToken = 0;
	let current = $state.raw<ScopedSearchBinding | undefined>(undefined);

	return {
		get active() {
			return current;
		},
		register(binding: ScopedSearchBinding) {
			const token = ++activeToken;
			current = binding;
			return () => {
				if (activeToken === token) {
					current = undefined;
				}
			};
		}
	};
}

export function setScopedSearchContext(host: ScopedSearchHost): ScopedSearchHost {
	return setContext(SCOPED_SEARCH_KEY, host);
}

export function useScopedSearchContext(): ScopedSearchHost | undefined {
	return getContext<ScopedSearchHost | undefined>(SCOPED_SEARCH_KEY);
}
