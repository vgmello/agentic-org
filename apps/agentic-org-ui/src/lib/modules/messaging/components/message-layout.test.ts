import { readFileSync } from 'node:fs';
import { expect, it } from 'vitest';
import { KIND_WORD } from '../model';

it('keeps the kind vocabulary within the MessageRow metadata width calibration', () => {
 // A new label requires remeasuring the widest intact icon+word group and the rem threshold,
 // then running the 16/32/40px boundary and four-width native browser checks.
 expect([...new Set(Object.values(KIND_WORD))].sort()).toEqual(['Announcement', 'Ask', 'Hand-off', 'Message', 'Reply']);
});

// Measured at a 32px root: intact kind, avatar, gap, padding and 4px safety.
// Round upward to 1/256rem; Tailwind query candidates must remain static literals.
const METADATA_THRESHOLD_REM = Math.ceil(((212.4375 + 72 + 24 + 32 + 4) / 32) * 256) / 256;
it('keeps every static metadata query at the named rem calibration', () => {
 const source = readFileSync(new URL('./MessageRow.svelte', import.meta.url), 'utf8');
 const queries = [...source.matchAll(/@max-\[([\d.]+)rem\]/g)].map((match) => Number(match[1]));
 expect(queries).toEqual(Array(3).fill(METADATA_THRESHOLD_REM));
});
