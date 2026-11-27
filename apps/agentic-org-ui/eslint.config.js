import js from '@eslint/js';
import { defineConfig } from 'eslint/config';
import prettier from 'eslint-config-prettier';
import svelte from 'eslint-plugin-svelte';
import globals from 'globals';
import ts from 'typescript-eslint';
import svelteConfig from './svelte.config.js';

const NO_RAW_FETCH_GLOBAL = {
	name: 'fetch',
	message: 'Route all authenticated HTTP/SSE calls through appSession.controller() (see .agents/rules/svelte-architecture-and-quality.md §4).'
};

const NO_RAW_LOCAL_STORAGE_GLOBAL = {
	name: 'localStorage',
	message: 'Use a caller-scoped device-store helper registered with appSession.onSessionEnded() (see .agents/rules/svelte-architecture-and-quality.md §4).'
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
	}
];

const NO_FETCH_MEMBER = [
	{
		selector: "MemberExpression[object.name=/^(?:window|globalThis)$/][property.name='fetch']",
		message: NO_RAW_FETCH_GLOBAL.message
	}
];

const NO_LOCAL_STORAGE_MEMBER = [
	{
		selector: "MemberExpression[object.name=/^(?:window|globalThis)$/][property.name='localStorage']",
		message: NO_RAW_LOCAL_STORAGE_GLOBAL.message
	}
];

/** Narrow legacy localStorage files allowed until migrated to a single caller-scoped device store. */
const LEGACY_LOCAL_STORAGE_FILES = [
	'src/lib/modules/inbox/device-comments.ts',
	'src/lib/modules/inbox/features/triage/InboxTriageWorkspace.svelte',
	'src/lib/modules/messaging/features/send-message/Composer.svelte',
	'src/lib/modules/work/features/requests/RequestsPanel.svelte',
	'src/lib/modules/work/features/tasks/TasksBoard.svelte',
	'src/lib/modules/workitems/filters.ts',
	'src/lib/platform/organizations/selection.ts',
	'src/lib/platform/shell/shortcuts.ts',
	'src/lib/platform/shell/ui-preferences.svelte.ts'
];

/** Sole transport choke-points permitted to call raw browser fetch(). */
const HTTP_TRANSPORT_FILES = [
	'src/lib/platform/auth/session-controller.ts',
	'src/lib/platform/http/events.ts',
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
	{
		files: ['src/lib/platform/**/*.{ts,js,svelte}', 'src/routes/(shell)/+layout.svelte'],
		ignores: ['**/*.test.ts', '**/*.spec.ts'],
		rules: {
			'no-restricted-imports': [
				'error',
				{
					patterns: [
						{
							regex: '^\\$lib/modules/[^/]+(?:/index(?:\\.[cm]?[jt]s)?)?$',
							message:
								'Platform/shell layout must not import top-level domain barrels (causes shell bundle bloat). Import non-UI subpaths or lazy-load UI components.'
						},
						{
							regex: '^(?:\\.\\./)+modules/[^/]+(?:/index(?:\\.[cm]?[jt]s)?)?$',
							message:
								'Platform/shell layout must not import top-level domain barrels via relative paths.'
						}
					]
				}
			]
		}
	},
	{
		files: [
			'src/lib/modules/**/*.{ts,js,svelte}',
			'src/lib/components/**/*.{ts,js,svelte}',
			'src/lib/platform/**/*.{ts,js,svelte}'
		],
		ignores: ['**/*.test.ts', '**/*.spec.ts', '**/testing/**', '**/mocks/**', '**/mock.ts'],
		rules: {
			'no-restricted-imports': [
				'error',
				{
					patterns: [
						{
							regex: '\\.css$',
							message:
								'Do not import plain .css files from modules, components, or platform — keep styles scoped inside cohesive Svelte components.'
						}
					]
				}
			]
		}
	},
	{
		files: ['src/lib/**/*.{ts,js,svelte}', 'src/routes/**/*.{ts,js,svelte}'],
		ignores: [
			'**/*.test.ts',
			'**/*.spec.ts',
			'**/testing/**',
			'**/mocks/**',
			'**/mock.ts',
			'**/fixtures/**',
			'**/sample.ts'
		],
		rules: {
			'no-restricted-globals': ['error', NO_RAW_FETCH_GLOBAL, NO_RAW_LOCAL_STORAGE_GLOBAL, NO_EVAL_GLOBAL],
			'no-restricted-syntax': ['error', ...NO_DOM_SINKS, ...NO_FETCH_MEMBER, ...NO_LOCAL_STORAGE_MEMBER]
		}
	},
	{
		files: HTTP_TRANSPORT_FILES,
		rules: {
			'no-restricted-globals': ['error', NO_RAW_LOCAL_STORAGE_GLOBAL, NO_EVAL_GLOBAL],
			'no-restricted-syntax': ['error', ...NO_DOM_SINKS, ...NO_LOCAL_STORAGE_MEMBER]
		}
	},
	{
		files: LEGACY_LOCAL_STORAGE_FILES,
		rules: {
			'no-restricted-globals': ['error', NO_RAW_FETCH_GLOBAL, NO_EVAL_GLOBAL],
			'no-restricted-syntax': ['error', ...NO_DOM_SINKS, ...NO_FETCH_MEMBER]
		}
	}
);
