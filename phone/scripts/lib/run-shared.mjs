// What the two run scripts share: running commands, generating the native project, Metro.

import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

export const phoneRoot = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
export const METRO_PORT = 8081;

export class RunError extends Error {}

/** `--release` and `--no-bundler`; anything else is a mistake. */
export function parseArgs(argv) {
  const options = { release: false, bundler: true };
  for (const arg of argv) {
    if (arg === '--release') options.release = true;
    else if (arg === '--no-bundler') options.bundler = false;
    else throw new RunError(`unknown argument: ${arg} (known: --release, --no-bundler)`);
  }
  // A release build carries its own JavaScript and never talks to Metro.
  if (options.release) options.bundler = false;
  return options;
}

/** Runs a command with its output shown. Throws when it fails. */
export function run(command, args, options = {}) {
  const result = spawnSync(command, args, { cwd: phoneRoot, stdio: 'inherit', ...options });
  if (result.error) throw new RunError(`${command} could not be started: ${result.error.message}`);
  if (result.status !== 0)
    throw new RunError(`${command} ${args.join(' ')} exited with ${result.status}`);
}

/** Runs a command and returns what it printed. Throws when it fails. */
export function read(command, args, options = {}) {
  const result = spawnSync(command, args, { cwd: phoneRoot, encoding: 'utf8', ...options });
  if (result.error) throw new RunError(`${command} could not be started: ${result.error.message}`);
  if (result.status !== 0) {
    throw new RunError(
      `${command} ${args.join(' ')} exited with ${result.status}: ${result.stderr.trim()}`,
    );
  }
  return result.stdout;
}

/** Like `read`, for commands that may fail without it mattering. */
export function attempt(command, args, options = {}) {
  const result = spawnSync(command, args, { cwd: phoneRoot, encoding: 'utf8', ...options });
  return {
    ok: !result.error && result.status === 0,
    output: `${result.stdout ?? ''}${result.stderr ?? ''}`,
  };
}

/**
 * Runs a build whose output is too long to read: everything goes to `logFile`, and only the
 * lines that say what failed are shown when it fails.
 */
export function runLogged(command, args, logFile, options = {}) {
  fs.mkdirSync(path.dirname(logFile), { recursive: true });
  const log = fs.openSync(logFile, 'w');
  let result;
  try {
    result = spawnSync(command, args, { cwd: phoneRoot, stdio: ['ignore', log, log], ...options });
  } finally {
    fs.closeSync(log);
  }
  const shown = path.relative(phoneRoot, logFile);
  if (result.error) throw new RunError(`${command} could not be started: ${result.error.message}`);
  if (result.status !== 0) {
    const failures = fs
      .readFileSync(logFile, 'utf8')
      .split('\n')
      .filter((line) => /(^|\s)(error:|FAILED|FAILURE:|\* What went wrong)/.test(line))
      .slice(0, 30);
    console.error(failures.join('\n'));
    throw new RunError(`${command} exited with ${result.status}. The whole output is in ${shown}`);
  }
  console.log(`Build output: ${shown}`);
}

/** Runs `work`, then says how long it took. */
export async function timed(label, work) {
  const started = Date.now();
  const result = await work();
  console.log(`${label}: ${((Date.now() - started) / 1000).toFixed(1)} s`);
  return result;
}

/**
 * The native project is generated, not kept (continuous native generation). It is generated
 * when it is missing; after a change to app.config.ts or to a native dependency, generate it
 * again with `npm run prebuild`.
 */
export function ensureNativeProject(platform) {
  if (fs.existsSync(path.join(phoneRoot, platform))) return;
  console.log(`${platform}/ is missing: generating it with expo prebuild`);
  run('npx', ['expo', 'prebuild', '--platform', platform], {
    // CocoaPods reads paths as UTF-8 and fails under a locale without an encoding.
    env: { ...process.env, EXPO_NO_TELEMETRY: '1', CI: '1', LANG: 'en_US.UTF-8', LC_ALL: 'en_US.UTF-8' },
  });
}

export async function isMetroRunning() {
  try {
    const response = await fetch(`http://127.0.0.1:${METRO_PORT}/status`, {
      signal: AbortSignal.timeout(1000),
    });
    return (await response.text()).includes('packager-status:running');
  } catch {
    return false;
  }
}

/**
 * Starts Metro unless it is running already, and resolves once it answers.
 * Returns the child process, or null when Metro was there before.
 */
export async function startMetro() {
  if (await isMetroRunning()) {
    console.log(`Metro is already running on port ${METRO_PORT}`);
    return null;
  }
  console.log(`Starting Metro on port ${METRO_PORT}`);
  const metro = spawn('npx', ['expo', 'start', '--port', String(METRO_PORT)], {
    cwd: phoneRoot,
    stdio: 'inherit',
    env: { ...process.env, EXPO_NO_TELEMETRY: '1' },
  });
  const deadline = Date.now() + 60_000;
  while (Date.now() < deadline) {
    if (metro.exitCode !== null) throw new RunError(`Metro exited with ${metro.exitCode}`);
    if (await isMetroRunning()) return metro;
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  metro.kill();
  throw new RunError('Metro did not answer within a minute');
}

/** Stays in the foreground with Metro until it ends or the script is interrupted. */
export function followMetro(metro) {
  if (metro === null) return Promise.resolve();
  console.log('Metro runs in the foreground. Stop it with Ctrl-C.');
  return new Promise((resolve) => {
    const stop = () => metro.kill('SIGINT');
    process.on('SIGINT', stop);
    process.on('SIGTERM', stop);
    metro.on('exit', resolve);
  });
}

/** Runs a script's main function and turns a failure into a message and an exit code. */
export function main(work) {
  work().catch((error) => {
    console.error(error instanceof RunError ? `error: ${error.message}` : error);
    process.exit(1);
  });
}
