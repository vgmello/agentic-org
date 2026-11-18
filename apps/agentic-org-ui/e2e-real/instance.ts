import { spawnSync } from 'node:child_process';
import {
	chmodSync,
	cpSync,
	mkdirSync,
	mkdtempSync,
	realpathSync,
	rmSync,
	writeFileSync
} from 'node:fs';
import { createServer } from 'node:net';
import { join, resolve } from 'node:path';
import { stopInstance } from './teardown';

const repo = resolve(import.meta.dirname, '../../..');

function freePort(): Promise<number> {
	return new Promise((done, fail) => {
		const server = createServer();
		server.once('error', fail);
		server.listen(0, '127.0.0.1', () => {
			const address = server.address();
			server.close(() =>
				address && typeof address === 'object' ? done(address.port) : fail(new Error('no port'))
			);
		});
	});
}

/** The first free loopback port at or after `base`: a spec with its own instance searches its own range. */
async function freePortFrom(base: number): Promise<number> {
	for (let port = base; port < base + 50; port++) {
		const free = await new Promise<boolean>((done) => {
			const server = createServer();
			server.once('error', () => done(false));
			server.listen(port, '127.0.0.1', () => server.close(() => done(true)));
		});
		if (free) return port;
	}
	throw new Error(`no free port from ${base} to ${base + 49}`);
}

/** The instance environment: everything under the disposable root, nothing inherited. */
export function instanceEnv(root: string, portBase?: number): NodeJS.ProcessEnv {
	return {
		// A named instance started from this root would search this spec's range, not the product's.
		...(portBase ? { ORG_DEV_NAMED_PORT_BASE: String(portBase + 100) } : {}),
		HOME: root,
		PATH: '/usr/bin:/bin',
		TMPDIR: `${root}/`,
		ORG_DEV_ACCOUNT_BASE: join(root, 'app'),
		ORG_OFFICE_DEV_KEY_DIR: join(root, 'keys'),
		ORG_DEV_SERVICE_DIR: join(root, 'svc'),
		ORG_DEV_DISK_ENCRYPTION: 'on'
	};
}

export interface Instance {
	root: string;
	base: string;
	link: string;
}

/**
 * Starts a real instance with `agentic-org setup` under a new disposable root and returns its
 * address and one-time setup link. `prefix` is a short name under /tmp; with `portBase` the web
 * port is the first free one from there, otherwise any free port. Stop it with `removeInstance`.
 */
export async function startInstance(prefix: string, portBase?: number): Promise<Instance> {
	// /tmp, not $TMPDIR: the instance's socket paths must fit the 71-byte temp budget.
	const root = realpathSync(mkdtempSync(`/tmp/${prefix}.`));
	chmodSync(root, 0o700);
	for (const dir of ['keys', 'svc', 'bin']) mkdirSync(join(root, dir), { mode: 0o700 });
	for (const bin of ['agentic-org', 'agentic-org-office', 'agentic-org-web']) {
		cpSync(join(repo, 'target/debug', bin), join(root, 'bin', bin));
	}
	cpSync(join(repo, 'apps/agentic-org-ui/build'), join(root, 'bin/ui'), { recursive: true });
	const port = portBase ? await freePortFrom(portBase) : await freePort();
	// `script` gives setup a terminal, as at a person's terminal: only then it prints the link (SIGN-22).
	const run = spawnSync(
		'/usr/bin/script',
		['-q', '/dev/null', join(root, 'bin/agentic-org'), 'setup', '--port', String(port)],
		{
			env: instanceEnv(root, portBase),
			stdio: ['ignore', 'pipe', 'pipe'],
			encoding: 'utf8',
			timeout: 120_000
		}
	);
	const output = `${run.stdout}\n--- stderr\n${run.stderr}`;
	const link = run.stdout.match(/http:\/\/127\.0\.0\.1:\d+\/register#[A-Za-z0-9_-]+/)?.[0];
	if (run.status !== 0 || !link) {
		// Do not rely on teardown after a failed setup: stop what setup started and remove the root here.
		stopInstance(root);
		rmSync(root, { recursive: true, force: true });
		delete process.env.B6_ROOT;
		throw new Error(
			`setup failed (status ${run.status}, signal ${run.signal ?? 'none'}${run.error ? `, ${run.error.message}` : ''}):\n${output.replace(/\/register#[A-Za-z0-9_-]+/g, '/register#<redacted>').slice(-4000)}`
		);
	}
	writeFileSync(join(root, 'setup.out'), output);
	return { root, base: `http://127.0.0.1:${port}`, link };
}

export default async function setup() {
	const instance = await startInstance('b6pw');
	process.env.B6_ROOT = instance.root;
	process.env.B6_BASE = instance.base;
	process.env.B6_LINK = instance.link;
}
