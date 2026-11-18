import { execFileSync } from 'node:child_process';
import { readdirSync, readFileSync, realpathSync, rmSync } from 'node:fs';
import { join } from 'node:path';

/** Stops every process started from the disposable root, then removes the root. */
export function stopInstance(root: string) {
	for (const name of readdirSync(join(root, 'svc'))) {
		if (!name.endsWith('.pid')) continue;
		const pid = Number(readFileSync(join(root, 'svc', name), 'utf8').trim());
		try {
			if (pid > 0) process.kill(pid, 'SIGTERM');
		} catch {
			/* Already gone. */
		}
	}
	const deadline = Date.now() + 10_000;
	while (Date.now() < deadline) {
		try {
			execFileSync('/usr/bin/pgrep', ['-f', root]);
		} catch {
			return;
		}
		execFileSync('/bin/sleep', ['0.2']);
	}
	try {
		execFileSync('/usr/bin/pkill', ['-KILL', '-f', root]);
	} catch {
		/* None left. */
	}
}

/** Stops and removes one disposable instance; refuses any root that is not one of ours under /tmp. */
export function removeInstance(root: string | undefined, prefix: string) {
	// instance.ts resolves /tmp: /private/tmp on macOS, /tmp itself on Linux.
	if (!root || !root.startsWith(`${realpathSync('/tmp')}/${prefix}.`)) return;
	stopInstance(root);
	rmSync(root, { recursive: true, force: true });
}

export default function teardown() {
	removeInstance(process.env.B6_ROOT, 'b6pw');
}
