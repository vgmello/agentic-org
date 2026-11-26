import js from '@eslint/js';
import { defineConfig } from 'eslint/config';
import prettier from 'eslint-config-prettier';
import svelte from 'eslint-plugin-svelte';
import globals from 'globals';
import ts from 'typescript-eslint';
import svelteConfig from './svelte.config.js';

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
		files: ['src/lib/platform/**/*.ts', 'src/lib/platform/**/*.svelte'],
		ignores: ['**/*.test.ts', '**/*.spec.ts'],
		rules: {
			'no-restricted-imports': [
				'error',
				{
					patterns: [
						{
							regex: '^\\$lib/modules/[^/]+$',
							message:
								'Platform shell code must not import top-level domain barrels (causes shell bundle bloat). Import specific non-UI subpaths or lazy-load UI components.'
						}
					]
				}
			]
		}
	}
);
