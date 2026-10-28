#!/usr/bin/env node
import { access, realpath } from 'node:fs/promises';
import { spawnSync } from 'node:child_process';
import { dirname, isAbsolute, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const directory = dirname(fileURLToPath(import.meta.url));
const root = resolve(directory, '../../..');
const cli = join(directory, 'node_modules/specification-guided-development/dist/cli.js');
const output = join(root, 'spec/sgd/generated/c1-report-domain');
const arguments_ = process.argv.slice(2);
const command = arguments_.shift() ?? 'verify';
if (!['verify', 'update'].includes(command)) throw new Error('Use verify or update');
let jar = join(root, 'specs/tla/tla2tools.jar');
if (arguments_.length !== 0) {
  if (arguments_.length !== 2 || arguments_[0] !== '--jar' || !arguments_[1]) {
    throw new Error('Usage: node verify.mjs [verify|update] [--jar FILE]');
  }
  jar = resolve(root, arguments_[1]);
}
const lean = process.env.SGD_LEAN_BINARY;
if (!lean || !isAbsolute(lean)) throw new Error('Set SGD_LEAN_BINARY to an absolute path to Lean 4.35.0');
try { await access(cli); } catch {
  throw new Error('Install the independently pinned report-domain SDK with npm ci in spec/sgd/report-domain');
}
const executable = await realpath(lean);
const result = spawnSync(process.execPath, [cli, 'build', '--project', join(directory, 'project.json'),
  '--jar', jar, '--lean', executable, '--out', output, ...(command === 'verify' ? ['--check'] : [])], {
  cwd: root, env: process.env, encoding: 'utf8', timeout: 600_000, maxBuffer: 32 * 1024 * 1024,
});
if (result.error) throw result.error;
if (result.status !== 0) throw new Error(`Report domain ${command} failed:\n${(result.stderr || result.stdout).slice(-16_384)}`);
console.log(`Report domain ${command} passed`);
