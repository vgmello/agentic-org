import { expect, it } from 'vitest';
import { KIND_WORD } from '../model';

it('keeps the kind vocabulary within the MessageRow metadata width calibration', () => {
 // A new label requires remeasuring the widest intact icon+word group and the rem threshold,
 // then running the 16/32/40px boundary and four-width native browser checks.
 expect([...new Set(Object.values(KIND_WORD))].sort()).toEqual(['Announcement', 'Ask', 'Hand-off', 'Message', 'Reply']);
});
