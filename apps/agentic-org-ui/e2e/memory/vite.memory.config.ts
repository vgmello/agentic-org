import { existsSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig, type Plugin } from 'vite';
import { testComposition } from '../vite-composition.ts';

// The probe copies this configuration into its ignored build sandbox. It cannot
// overwrite the ordinary application's release build or select runtime fakes there.
if (!existsSync(resolve('.memory-build-sandbox')))
	throw new Error('Run e2e/memory/probe.ts; this config requires its isolated build sandbox.');
const seed = fileURLToPath(new URL('./seed.ts', import.meta.url));
const memorySeed: Plugin = {
	name: 'memory-test-seed',
	enforce: 'pre',
	resolveId(source, importer) {
		if (
			source === './chat-sample' &&
			importer?.split('?')[0].endsWith('/e2e/fakes/modules/chat.ts')
		)
			return seed;
	}
};
export default defineConfig({
	plugins: [tailwindcss(), sveltekit(), testComposition(), memorySeed],
	build: { assetsInlineLimit: 0 }
});
