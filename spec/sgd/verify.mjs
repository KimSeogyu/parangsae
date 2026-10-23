#!/usr/bin/env node
import { execFile } from 'node:child_process';
import { createHash } from 'node:crypto';
import { access, link, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { promisify } from 'node:util';

const execute = promisify(execFile);
const directory = dirname(fileURLToPath(import.meta.url));
const root = resolve(directory, '../..');
const sdk = join(directory, 'node_modules/specification-guided-development');
const cli = join(sdk, 'dist/cli.js');
const defaultJar = join(root, 'specs/tla/tla2tools.jar');
const officialJar = 'https://github.com/tlaplus/tlaplus/releases/download/v1.7.4/tla2tools.jar';
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const args = process.argv.slice(2);
const command = args.shift() ?? 'verify';
let jar = defaultJar;
if (args.length) {
  if (args.length !== 2 || args[0] !== '--jar' || !args[1]) throw new Error('Usage: node spec/sgd/verify.mjs [verify|update|fetch-tlc] [--jar FILE]');
  // A relative --jar always refers to the application root, including npm --prefix runs.
  jar = resolve(root, args[1]);
}

async function step(label, executable, arguments_) {
  process.stdout.write(`${label}...\n`);
  try {
    await execute(executable, arguments_, {
      cwd: root,
      timeout: 600_000,
      maxBuffer: 32 * 1024 * 1024,
      env: process.env,
    });
  } catch (error) {
    const detail = error && typeof error === 'object' && 'stderr' in error ? String(error.stderr).trim() : String(error);
    throw new Error(`${label} failed: ${detail.slice(-16_384) || String(error)}`);
  }
}

async function checkedJar() {
  const { FINITE_TLC_JAR_SHA256 } = await import(pathToFileURL(join(sdk, 'dist/finite-tlc.js')).href);
  let bytes;
  try { bytes = await readFile(jar); }
  catch (error) {
    if (command !== 'fetch-tlc' || !error || typeof error !== 'object' || !('code' in error) || error.code !== 'ENOENT') {
      throw new Error(`Cannot read TLC JAR at ${jar}. Run npm --prefix spec/sgd run fetch-tlc, or pass --jar FILE.`);
    }
    const response = await fetch(officialJar, { signal: AbortSignal.timeout(60_000) });
    if (!response.ok || !response.body) throw new Error(`TLC download failed with HTTP ${response.status}`);
    const chunks = [];
    let size = 0;
    for await (const chunk of response.body) {
      size += chunk.byteLength;
      if (size > 16 * 1024 * 1024) throw new Error('TLC download exceeded 16 MiB');
      chunks.push(chunk);
    }
    bytes = Buffer.concat(chunks);
    if (digest(bytes) !== FINITE_TLC_JAR_SHA256) throw new Error('Downloaded TLC JAR does not match the SDK pin; no file was published');
    await mkdir(dirname(jar), { recursive: true });
    const temporary = `${jar}.sgd-download-${process.pid}`;
    try {
      await writeFile(temporary, bytes, { flag: 'wx' });
      // Publish without overwriting a file another process created during the download.
      try {
        await link(temporary, jar);
      } catch (error) {
        if (!error || typeof error !== 'object' || !('code' in error) || error.code !== 'EEXIST') throw error;
        const current = await readFile(jar);
        if (digest(current) !== FINITE_TLC_JAR_SHA256) throw new Error(`A different TLC JAR already exists at ${jar}`);
      }
    } finally { await rm(temporary, { force: true }); }
  }
  if (digest(bytes) !== FINITE_TLC_JAR_SHA256) throw new Error(`TLC JAR at ${jar} does not match the installed SDK pin`);
}

async function main() {
  if (!['verify', 'update', 'fetch-tlc'].includes(command)) throw new Error(`Unknown command ${command}`);
  if (Number(process.versions.node.split('.')[0]) < 22) throw new Error('Node.js 22 or later is required');
  try { await access(cli); }
  catch { throw new Error('The pinned SDK is not prepared. Run npm --prefix spec/sgd ci first, with lifecycle scripts enabled.'); }
  await checkedJar();
  if (command === 'fetch-tlc') {
    process.stdout.write(`Pinned TLC JAR is ready at ${jar}\n`);
    return;
  }
  const generated = join(directory, 'generated');
  const concrete = join(generated, 'fact-write');
  const abstract = join(generated, 'manifest-write');
  const check = command === 'verify' ? ['--check'] : [];
  for (const [label, project, out] of [
    ['Check source-bound FactWriteEnvelope', 'fact-write.json', concrete],
    ['Check original ManifestWrite', 'manifest-write.json', abstract],
  ]) {
    await step(label, process.execPath, [cli, 'build', '--project', join(directory, project), '--jar', jar, '--out', out, ...check]);
  }
  await step('Check payload-contract erasure', process.execPath, [cli, 'refine',
    '--abstract', join(abstract, 'symbolic-model.json'), '--concrete', join(concrete, 'symbolic-model.json'),
    '--binding', join(directory, 'fact-erasure.json'), '--out', join(generated, 'fact-erasure'), ...check]);
  const requireSdk = createRequire(join(sdk, 'package.json'));
  const typescriptPackage = requireSdk.resolve('typescript/package.json');
  const typescript = JSON.parse(await readFile(typescriptPackage, 'utf8'));
  if (typescript.version !== '7.0.2') throw new Error(`The SDK must supply TypeScript 7.0.2; resolved ${typescript.version}`);
  if (typeof typescript.bin?.tsc !== 'string') throw new Error('The installed TypeScript package has no tsc entry point');
  const tsc = resolve(dirname(typescriptPackage), typescript.bin.tsc);
  for (const [label, out] of [['whole-row', concrete], ['abstract', abstract]]) {
    await step(`Check generated ${label} TypeScript 7.0.2 without emission`, process.execPath,
      [tsc, '--noEmit', '--project', join(out, 'typescript/tsconfig.json')]);
  }
  const view = join(root, '.runtime/sgd-view');
  await step('Render fresh Pele witness and symbolic views', process.execPath, [cli, 'visualize',
    '--ir', join(abstract, 'automaton.json'), '--ir', join(abstract, 'symbolic-model.json'),
    '--ir', join(concrete, 'automaton.json'), '--ir', join(concrete, 'symbolic-model.json'), '--out', view]);
  process.stdout.write(`${command === 'verify' ? 'Verified existing' : 'Updated'} source/model bindings, original payload contract, erasure, and TypeScript output.\n`);
  process.stdout.write(`Fresh Pele view: ${join(view, 'index.html')}\n`);
  process.stdout.write('Scope: per-live-owner whole-row authority. Partition effects, persistence, all-target coverage, and required-CI activation are separate.\n');
}

main().catch(error => {
  process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
  process.exitCode = 1;
});
