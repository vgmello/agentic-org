import { afterEach, expect, it } from 'vitest';
import { chmod, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { spawn, spawnSync } from 'node:child_process';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { execute } from '../../../scripts/csp-gateway';

const runner = fileURLToPath(new URL('../../../scripts/csp-gateway.ts', import.meta.url));
const roots: string[] = [];
afterEach(async () => { await Promise.all(roots.splice(0).map((path) => rm(path, { recursive: true, force: true }))); });
async function directory() { const path = await mkdtemp(join(tmpdir(), 'org-csp-process-')); roots.push(path); return path; }
async function executable(path: string, source: string) {
	await writeFile(path, `#!${process.execPath}\n${source}`);
	await chmod(path, 0o700);
}

it('refuses serving without the freshly built executable instead of searching PATH', async () => {
	const root = await directory();
	await executable(join(root, 'cargo'), 'process.exit(17);');
	const env: NodeJS.ProcessEnv = { ...process.env, CSP_GW_PORT: '4273', PATH: `${root}:${process.env.PATH}` }; delete env.CSP_GW_EXECUTABLE;
	const result = spawnSync('bun', [runner, 'serve'], { env, encoding: 'utf8' });
	expect(result.status).toBe(1);
	expect(result.stderr).toContain('Run bun run test:e2e:csp-gateway to build the gateway first');
});

it('preserves a split UTF-8 character in captured Cargo artifact stdout', async () => {
	const root = await directory();
	const cargo = join(root, 'cargo');
	const artifact = { reason: 'compiler-artifact', target: { name: 'csp_harness', kind: ['example'] }, executable: '/tmp/ü-target/csp_harness' };
	const line = JSON.stringify(artifact) + '\n';
	await executable(cargo, `const fs=require('node:fs');const bytes=Buffer.from(${JSON.stringify(line)});const split=bytes.indexOf(0xc3)+1;fs.writeSync(1,bytes.subarray(0,split));setTimeout(()=>fs.writeSync(1,bytes.subarray(split)),50);`);
	expect(await execute(cargo, [], root, process.env, true)).toBe(line);
});

it('stops on a failed Cargo build even if stdout contains an executable artifact', async () => {
	const root = await directory();
	const artifact = { reason: 'compiler-artifact', target: { name: 'csp_harness', kind: ['example'] }, executable: '/bin/true' };
	await executable(join(root, 'cargo'), `require('node:fs').writeSync(1,${JSON.stringify(JSON.stringify(artifact)+'\n')});process.exit(17);`);
	const result = spawnSync('bun', [runner, '--list'], { env: { ...process.env, CSP_GW_PORT: '4273', PATH: `${root}:${process.env.PATH}` }, encoding: 'utf8', timeout: 55_000 });
	expect(result.error).toBeUndefined();
	expect(result.status).toBe(1);
	expect(result.stderr).toContain('cargo exited 17');
}, 60_000);

for (const signal of ['SIGTERM', 'SIGINT'] as const) {
	it(`forwards ${signal} to its exact server child and waits for that child to stop`, async () => {
		const root = await directory();
		const pidFile = join(root, 'child.pid');
		const doneFile = join(root, 'child.done');
		const binary = join(root, 'server');
		await executable(binary, `const fs=require('node:fs');fs.writeFileSync(${JSON.stringify(pidFile)},String(process.pid));for(const s of ['SIGTERM','SIGINT'])process.on(s,()=>{fs.writeFileSync(${JSON.stringify(doneFile)},s);process.exit(0)});setInterval(()=>{},1000);`);
		const parent = spawn('bun', [runner, 'serve'], { env: { ...process.env, CSP_GW_PORT: '4273', CSP_GW_EXECUTABLE: binary }, stdio: 'ignore' });
		const finished = new Promise<number | null>((resolve, reject) => { parent.once('error', reject); parent.once('close', resolve); });
		let child: number | undefined;
		try {
			await expect.poll(() => readFile(pidFile, 'utf8').catch(() => ''), { timeout: 5_000 }).toMatch(/^\d+$/);
			child = Number(await readFile(pidFile, 'utf8'));
			parent.kill(signal);
			expect(await finished).toBe(0);
			expect(await readFile(doneFile, 'utf8')).toBe(signal);
		} finally {
			// The fixture never exits on its own; a failed forwarding mutant must not leave it alive.
			if (child && !await readFile(doneFile, 'utf8').catch(() => '')) try { process.kill(child, 'SIGKILL'); } catch { /* already stopped */ }
			if (parent.exitCode === null && parent.signalCode === null) parent.kill('SIGKILL');
		}
	}, 10_000);
}
