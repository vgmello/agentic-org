import { describe, expect, it } from 'vitest';
import { createSlugField, slugFromName, validSlug } from './slug';

describe('slugFromName', () => {
	it.each([
		['Side Project', 'side-project'],
		['  Acme,  Inc.  ', 'acme-inc'],
		['Café Zürich', 'cafe-zurich'],
		['R&D -- 2026', 'r-d-2026'],
		['日本', ''],
		['', '']
	])('%j becomes %j', (name, slug) => {
		expect(slugFromName(name)).toBe(slug);
		if (slug) expect(validSlug(slug)).toBe(true);
	});
	it('never offers more than 64 characters or a trailing hyphen', () => {
		const slug = slugFromName(`${'a'.repeat(63)} b c`);
		expect(slug).toBe('a'.repeat(63));
		expect(validSlug(slugFromName('x'.repeat(200)))).toBe(true);
	});
});

describe('validSlug', () => {
	it.each(['a', 'side-project', 'a1-b2', 'a'.repeat(64)])('accepts %j', (slug) =>
		expect(validSlug(slug)).toBe(true)
	);
	it.each(['', 'Side', 'a--b', '-a', 'a-', 'a b', 'a_b', 'é', 'a'.repeat(65)])(
		'refuses %j',
		(slug) => expect(validSlug(slug)).toBe(false)
	);
});

describe('slug field', () => {
	it('follows the name until the person edits the slug', () => {
		const field = createSlugField();
		expect(field.nameChanged('Side Project')).toBe('side-project');
		expect(field.slugEdited('sp', 'Side Project')).toBe('sp');
		expect(field.nameChanged('Side Project Two')).toBe('sp');
		expect(field.edited).toBe(true);
	});
	it('keeps what the person typed, valid or not: the form reports it', () => {
		const field = createSlugField();
		expect(field.slugEdited('Not Kebab', 'Name')).toBe('Not Kebab');
	});
	it('a cleared slug follows the name again', () => {
		const field = createSlugField();
		field.slugEdited('sp', 'Side Project');
		expect(field.slugEdited('', 'Side Project')).toBe('side-project');
		expect(field.nameChanged('Other')).toBe('other');
	});
});
