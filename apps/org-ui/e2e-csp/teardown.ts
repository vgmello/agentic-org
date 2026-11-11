import { clean } from './collector';

export default async function teardown() {
	await new Promise((resolve) => setTimeout(resolve, 3000));
	await clean();
}
