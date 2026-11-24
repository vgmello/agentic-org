export async function load({ parent }) {
	const { connection } = await parent();
	return { connection };
}
