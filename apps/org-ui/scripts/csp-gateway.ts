import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const app = fileURLToPath(new URL('../', import.meta.url));
const repository = fileURLToPath(new URL('../../../', import.meta.url));

export function gatewayPort(value?: string): number {
	const port = Number(value ?? '4273');
	if (!Number.isInteger(port) || port < 1 || port > 65535) throw new Error('Invalid CSP_GW_PORT');
	return port;
}

export function gatewayExecutable(output: string): string {
	for (const line of output.split('\n').filter((line) => line.trim())) {
		const message = JSON.parse(line);
		if (message.reason === 'compiler-artifact' && message.target?.name === 'csp_harness'
			&& message.target?.kind?.includes('example') && typeof message.executable === 'string' && message.executable) return message.executable;
	}
	throw new Error('Cargo did not report the csp_harness executable');
}

function execute(command: string, args: string[], cwd: string, env = process.env, capture = false): Promise<string> {
	return new Promise((resolve, reject) => {
		const child = spawn(command, args, { cwd, env, stdio: ['ignore', capture ? 'pipe' : 'inherit', 'inherit'] });
		let output = '';
		child.stdout?.setEncoding('utf8');
		child.stdout?.on('data', (chunk) => { output += chunk; });
		const interrupt = () => child.kill('SIGINT');
		const terminate = () => child.kill('SIGTERM');
		process.once('SIGINT', interrupt);
		process.once('SIGTERM', terminate);
		const cleanup = () => { process.off('SIGINT', interrupt); process.off('SIGTERM', terminate); };
		child.once('error', (error) => { cleanup(); reject(error); });
		child.once('close', (code, signal) => {
			cleanup();
			if (code === 0) resolve(output);
			else reject(new Error(`${command} exited ${code ?? signal}`));
		});
	});
}

if (import.meta.main) {
	try {
		const port = String(gatewayPort(process.env.CSP_GW_PORT));
		if (process.argv[2] === 'serve') {
			const executable = process.env.CSP_GW_EXECUTABLE;
			if (!executable) throw new Error('Run bun run test:e2e:csp-gateway to build the gateway first');
			await execute(executable, ['--ui-build', `${app}build`, '--port', port], repository);
		} else {
			const output = await execute('cargo', ['build', '--locked', '-p', 'org-gateway', '--example', 'csp_harness', '--message-format=json'], repository, { ...process.env, CARGO_BUILD_JOBS: '2' }, true);
			const executable = gatewayExecutable(output);
			await execute(process.execPath, ['run', 'build'], app);
			await execute(process.execPath, ['x', 'playwright', 'test', '--config', 'playwright.csp-gateway.config.ts', ...process.argv.slice(2)], app, { ...process.env, CSP_GW_PORT: port, CSP_GW_EXECUTABLE: executable });
		}
	} catch (error) {
		console.error(error);
		process.exitCode = 1;
	}
}
