// 21rem reserves 4rem for 6ch of name, 9.375rem for the widest pill (7rem label +
// 2.375rem icon/gap/padding), 1.25rem chevron, 2rem gaps and 4rem gutters, rounded up.
// Scale with the list font so enlarged text cannot leave a tiny name or an icon-only pill.
export const compactRow = {
	gutters: '@max-[21rem]:px-[min(1rem,5cqi)]',
	name: '@max-[21rem]:w-full @max-[21rem]:flex-none',
	gap: '@max-[21rem]:gap-x-2',
	chevron: '@max-[21rem]:ml-0',
	bleed: '@max-[21rem]:-mx-[min(1rem,5cqi)]'
};

/** The page's gutters, which the full-width divided rows bleed back out of. */
export const listBleed = `${compactRow.bleed} -mx-4 sm:-mx-6 lg:-mx-8`;
