import js from '@eslint/js';
import { defineConfig } from 'eslint/config';
import prettier from 'eslint-config-prettier';
import svelte from 'eslint-plugin-svelte';
import globals from 'globals';
import ts from 'typescript-eslint';
import svelteConfig from './svelte.config.js';

const NO_RAW_FETCH_GLOBAL = {
	name: 'fetch',
	message:
		'Route all authenticated HTTP/SSE calls through appSession.controller() (see .agents/rules/svelte-architecture-and-quality.md §4).'
};

const NO_RAW_EVENT_SOURCE_GLOBAL = {
	name: 'EventSource',
	message:
		'Route all SSE streams through appSession.controller().openSseStream() / subscribeEvents() (EventSource cannot send X-Org-Session-Proof).'
};

const NO_RAW_LOCAL_STORAGE_GLOBAL = {
	name: 'localStorage',
	message:
		'Use a caller-scoped device-store helper registered with appSession.onSessionEnded() (see .agents/rules/svelte-architecture-and-quality.md §4).'
};

const NO_RAW_SESSION_STORAGE_GLOBAL = {
	name: 'sessionStorage',
	message:
		'Use a caller-scoped device-store helper registered with appSession.onSessionEnded() (see .agents/rules/svelte-architecture-and-quality.md §4).'
};

const NO_EVAL_GLOBAL = {
	name: 'eval',
	message: 'Never use eval().'
};

const NO_DOM_SINKS = [
	{
		selector: "MemberExpression[property.name='innerHTML']",
		message: 'Never assign or read innerHTML; render structured text via Svelte templates.'
	},
	{
		selector: "MemberExpression[property.name='outerHTML']",
		message: 'Never assign outerHTML; render structured text via Svelte templates.'
	},
	{
		selector: "CallExpression[callee.property.name='insertAdjacentHTML']",
		message: 'Never call insertAdjacentHTML; render structured text via Svelte templates.'
	},
	{
		selector: "CallExpression[callee.name='eval']",
		message: 'Never call eval().'
	},
	{
		selector: "NewExpression[callee.name='Function']",
		message: 'Never construct dynamic functions with new Function().'
	}
];

const NO_FETCH_MEMBER = [
	{
		selector: "MemberExpression[object.name=/^(?:window|globalThis|self)$/][property.name='fetch']",
		message: NO_RAW_FETCH_GLOBAL.message
	},
	{
		selector:
			"VariableDeclarator[init.name=/^(?:window|globalThis|self)$/] > ObjectPattern > Property[key.name='fetch']",
		message: NO_RAW_FETCH_GLOBAL.message
	}
];

const NO_EVENT_SOURCE_MEMBER = [
	{
		selector:
			"MemberExpression[object.name=/^(?:window|globalThis|self)$/][property.name='EventSource']",
		message: NO_RAW_EVENT_SOURCE_GLOBAL.message
	},
	{
		selector:
			"VariableDeclarator[init.name=/^(?:window|globalThis|self)$/] > ObjectPattern > Property[key.name='EventSource']",
		message: NO_RAW_EVENT_SOURCE_GLOBAL.message
	}
];

const NO_LOCAL_STORAGE_MEMBER = [
	{
		selector:
			"MemberExpression[object.name=/^(?:window|globalThis|self)$/][property.name='localStorage']",
		message: NO_RAW_LOCAL_STORAGE_GLOBAL.message
	},
	{
		selector:
			"VariableDeclarator[init.name=/^(?:window|globalThis|self)$/] > ObjectPattern > Property[key.name='localStorage']",
		message: NO_RAW_LOCAL_STORAGE_GLOBAL.message
	}
];

const NO_SESSION_STORAGE_MEMBER = [
	{
		selector:
			"MemberExpression[object.name=/^(?:window|globalThis|self)$/][property.name='sessionStorage']",
		message: NO_RAW_SESSION_STORAGE_GLOBAL.message
	},
	{
		selector:
			"VariableDeclarator[init.name=/^(?:window|globalThis|self)$/] > ObjectPattern > Property[key.name='sessionStorage']",
		message: NO_RAW_SESSION_STORAGE_GLOBAL.message
	}
];

const NO_SESSION_PROOF_LITERAL = [
	{
		selector: 'Literal[value=/^x-org-session-proof$/i]',
		message:
			'Never hand-roll X-Org-Session-Proof outside the platform transport choke-points (session-controller.ts, events.ts, json-http.ts, wire.ts).'
	}
];

const CSS_IMPORT_PATTERNS = [
	{
		regex: '\\.css(?:[?#].*)?$',
		message:
			'Do not import plain .css files from modules, components, platform, or interior routes — keep styles scoped inside cohesive Svelte components.'
	}
];

const BARREL_IMPORT_PATTERNS = [
	{
		regex: '^\\$lib/modules/[^/]+(?:/index(?:\\.[cm]?[jt]s)?)?$',
		message:
			'Platform/shell layout must not import top-level domain barrels (causes shell bundle bloat). Import non-UI subpaths or lazy-load UI components.'
	},
	{
		regex: '^(?:\\.\\./)+modules/[^/]+(?:/index(?:\\.[cm]?[jt]s)?)?$',
		message: 'Platform/shell layout must not import top-level domain barrels via relative paths.'
	}
];

const SESSION_PROOF_IMPORT_PATHS = [
	{
		name: '$lib/platform/http/wire',
		importNames: ['SESSION_PROOF_HEADER'],
		message:
			'SESSION_PROOF_HEADER may only be imported by transport choke-points (session-controller.ts, events.ts, json-http.ts).'
	},
	{
		name: './wire',
		importNames: ['SESSION_PROOF_HEADER'],
		message:
			'SESSION_PROOF_HEADER may only be imported by transport choke-points (session-controller.ts, events.ts, json-http.ts).'
	},
	{
		name: '../http/wire',
		importNames: ['SESSION_PROOF_HEADER'],
		message:
			'SESSION_PROOF_HEADER may only be imported by transport choke-points (session-controller.ts, events.ts, json-http.ts).'
	}
];

const NON_PRODUCTION_IGNORES = [
	'**/*.test.ts',
	'**/*.spec.ts',
	'**/testing/**',
	'**/mocks/**',
	'**/mock.ts',
	'**/fixtures/**',
	'**/sample.ts'
];

/** Narrow legacy localStorage files allowed until migrated to a single caller-scoped device store. */
const LEGACY_LOCAL_STORAGE_FILES = [
	'src/lib/modules/inbox/features/triage/InboxTriageWorkspace.svelte',
	'src/lib/modules/messaging/features/send-message/Composer.svelte',
	'src/lib/modules/work/features/requests/RequestsPanel.svelte',
	'src/lib/modules/work/features/tasks/TasksBoard.svelte',
	'src/lib/modules/workitems/filters.ts',
	'src/lib/platform/organizations/selection.ts',
	'src/lib/platform/shell/shortcuts.ts',
	'src/lib/platform/shell/ui-preferences.svelte.ts'
];

/** Caller-scoped device storage modules permitted to use localStorage + sessionStorage. */
const DEVICE_STORAGE_FILES = ['src/lib/modules/inbox/device-comments.ts'];

/** Narrow legacy sessionStorage-only files. */
const LEGACY_SESSION_STORAGE_FILES = ['src/lib/modules/workitems/archive.ts'];

/** Sole transport choke-points permitted to call raw browser fetch() and use SESSION_PROOF_HEADER. */
const HTTP_TRANSPORT_FILES = [
	'src/lib/platform/auth/session-controller.ts',
	'src/lib/platform/http/events.ts',
	'src/lib/platform/http/json-http.ts'
];

/** Files allowed to define or reference the literal 'X-Org-Session-Proof' header name. */
const SESSION_PROOF_LITERAL_FILES = [
	'src/lib/platform/http/wire.ts',
	'src/lib/platform/http/json-http.ts'
];

export default defineConfig(
	{
		ignores: [
			'node_modules/**',
			'.svelte-kit/**',
			'build/**',
			'dist/**',
			'test-results*/**',
			'playwright-report/**',
			'.claude/**'
		]
	},
	js.configs.recommended,
	ts.configs.recommended,
	svelte.configs.recommended,
	prettier,
	svelte.configs.prettier,
	{
		languageOptions: { globals: { ...globals.browser, ...globals.node, ...globals.bun } }
	},
	{
		files: ['**/*.svelte', '**/*.svelte.ts', '**/*.svelte.js'],
		languageOptions: {
			parserOptions: { parser: ts.parser, svelteConfig }
		}
	},
	{
		files: ['**/*.svelte'],
		rules: {
			'svelte/button-has-type': 'error',
			'svelte/no-target-blank': 'error'
		}
	},
	// Typed async promise hygiene for production TypeScript & Svelte rune modules.
	{
		files: ['src/**/*.ts'],
		ignores: [...NON_PRODUCTION_IGNORES, '**/*.svelte.ts'],
		languageOptions: {
			parser: ts.parser,
			parserOptions: { projectService: true, tsconfigRootDir: import.meta.dirname }
		},
		rules: {
			'@typescript-eslint/no-floating-promises': 'error',
			'@typescript-eslint/no-misused-promises': [
				'error',
				{ checksVoidReturn: { attributes: false } }
			]
		}
	},
	{
		files: ['src/**/*.svelte.ts'],
		ignores: NON_PRODUCTION_IGNORES,
		languageOptions: {
			parserOptions: {
				parser: ts.parser,
				svelteConfig,
				projectService: true,
				tsconfigRootDir: import.meta.dirname
			}
		},
		rules: {
			'@typescript-eslint/no-floating-promises': 'error',
			'@typescript-eslint/no-misused-promises': [
				'error',
				{ checksVoidReturn: { attributes: false } }
			]
		}
	},
	// Import restrictions: general production scope (bans plain .css imports & SESSION_PROOF_HEADER).
	{
		files: ['src/lib/**/*.{ts,js,svelte}', 'src/routes/**/*.{ts,js,svelte}'],
		ignores: [...NON_PRODUCTION_IGNORES, 'src/lib/styles/**', 'src/routes/+layout.svelte'],
		rules: {
			'no-restricted-imports': [
				'error',
				{
					paths: SESSION_PROOF_IMPORT_PATHS,
					patterns: CSS_IMPORT_PATTERNS
				}
			]
		}
	},
	// Platform & (shell)/+layout.svelte: explicitly merges barrel ban + CSS ban + SESSION_PROOF_HEADER ban (never overwritten!).
	{
		files: ['src/lib/platform/**/*.{ts,js,svelte}', 'src/routes/(shell)/+layout.svelte'],
		ignores: NON_PRODUCTION_IGNORES,
		rules: {
			'no-restricted-imports': [
				'error',
				{
					paths: SESSION_PROOF_IMPORT_PATHS,
					patterns: [...BARREL_IMPORT_PATTERNS, ...CSS_IMPORT_PATTERNS]
				}
			]
		}
	},
	// Transport choke-points may import SESSION_PROOF_HEADER while keeping barrel + CSS bans.
	{
		files: HTTP_TRANSPORT_FILES,
		rules: {
			'no-restricted-imports': [
				'error',
				{
					patterns: [...BARREL_IMPORT_PATTERNS, ...CSS_IMPORT_PATTERNS]
				}
			]
		}
	},
	// Security globals & syntax: default production scope.
	{
		files: ['src/lib/**/*.{ts,js,svelte}', 'src/routes/**/*.{ts,js,svelte}'],
		ignores: NON_PRODUCTION_IGNORES,
		rules: {
			'no-restricted-globals': [
				'error',
				NO_RAW_FETCH_GLOBAL,
				NO_RAW_EVENT_SOURCE_GLOBAL,
				NO_RAW_LOCAL_STORAGE_GLOBAL,
				NO_RAW_SESSION_STORAGE_GLOBAL,
				NO_EVAL_GLOBAL
			],
			'no-restricted-syntax': [
				'error',
				...NO_DOM_SINKS,
				...NO_FETCH_MEMBER,
				...NO_EVENT_SOURCE_MEMBER,
				...NO_LOCAL_STORAGE_MEMBER,
				...NO_SESSION_STORAGE_MEMBER,
				...NO_SESSION_PROOF_LITERAL
			]
		}
	},
	// Transport choke-points: allow raw fetch() only (still ban EventSource, storage, DOM sinks, proof literal).
	{
		files: HTTP_TRANSPORT_FILES,
		rules: {
			'no-restricted-globals': [
				'error',
				NO_RAW_EVENT_SOURCE_GLOBAL,
				NO_RAW_LOCAL_STORAGE_GLOBAL,
				NO_RAW_SESSION_STORAGE_GLOBAL,
				NO_EVAL_GLOBAL
			],
			'no-restricted-syntax': [
				'error',
				...NO_DOM_SINKS,
				...NO_EVENT_SOURCE_MEMBER,
				...NO_LOCAL_STORAGE_MEMBER,
				...NO_SESSION_STORAGE_MEMBER,
				...NO_SESSION_PROOF_LITERAL
			]
		}
	},
	// wire.ts & json-http.ts: sole files allowed to hold the 'X-Org-Session-Proof' literal.
	{
		files: SESSION_PROOF_LITERAL_FILES,
		rules: {
			'no-restricted-syntax': [
				'error',
				...NO_DOM_SINKS,
				...NO_EVENT_SOURCE_MEMBER,
				...NO_LOCAL_STORAGE_MEMBER,
				...NO_SESSION_STORAGE_MEMBER
			]
		}
	},
	// wire.ts must still ban raw fetch().
	{
		files: ['src/lib/platform/http/wire.ts'],
		rules: {
			'no-restricted-syntax': [
				'error',
				...NO_DOM_SINKS,
				...NO_FETCH_MEMBER,
				...NO_EVENT_SOURCE_MEMBER,
				...NO_LOCAL_STORAGE_MEMBER,
				...NO_SESSION_STORAGE_MEMBER
			]
		}
	},
	// Legacy localStorage-only files.
	{
		files: LEGACY_LOCAL_STORAGE_FILES,
		rules: {
			'no-restricted-globals': [
				'error',
				NO_RAW_FETCH_GLOBAL,
				NO_RAW_EVENT_SOURCE_GLOBAL,
				NO_RAW_SESSION_STORAGE_GLOBAL,
				NO_EVAL_GLOBAL
			],
			'no-restricted-syntax': [
				'error',
				...NO_DOM_SINKS,
				...NO_FETCH_MEMBER,
				...NO_EVENT_SOURCE_MEMBER,
				...NO_SESSION_STORAGE_MEMBER,
				...NO_SESSION_PROOF_LITERAL
			]
		}
	},
	// Caller-scoped device storage modules (localStorage + sessionStorage).
	{
		files: DEVICE_STORAGE_FILES,
		rules: {
			'no-restricted-globals': [
				'error',
				NO_RAW_FETCH_GLOBAL,
				NO_RAW_EVENT_SOURCE_GLOBAL,
				NO_EVAL_GLOBAL
			],
			'no-restricted-syntax': [
				'error',
				...NO_DOM_SINKS,
				...NO_FETCH_MEMBER,
				...NO_EVENT_SOURCE_MEMBER,
				...NO_SESSION_PROOF_LITERAL
			]
		}
	},
	// Legacy sessionStorage-only files.
	{
		files: LEGACY_SESSION_STORAGE_FILES,
		rules: {
			'no-restricted-globals': [
				'error',
				NO_RAW_FETCH_GLOBAL,
				NO_RAW_EVENT_SOURCE_GLOBAL,
				NO_RAW_LOCAL_STORAGE_GLOBAL,
				NO_EVAL_GLOBAL
			],
			'no-restricted-syntax': [
				'error',
				...NO_DOM_SINKS,
				...NO_FETCH_MEMBER,
				...NO_EVENT_SOURCE_MEMBER,
				...NO_LOCAL_STORAGE_MEMBER,
				...NO_SESSION_PROOF_LITERAL
			]
		}
	}
);
