import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vitest/config';
import { testComposition } from './e2e/vite-composition.ts';

// Fakes are a test-owned dev-server composition. Every build (including --mode e2e) stays unavailable.
export default defineConfig(({ command, mode, isPreview }) => ({
	plugins: [tailwindcss(), sveltekit(), ...(command === 'serve' && mode === 'e2e' && !isPreview ? [testComposition()] : [])],
	test: { include: ['src/**/*.{test,spec}.{js,ts}'], environment: 'node' }
}));
