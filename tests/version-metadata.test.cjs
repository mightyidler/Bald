const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const test = require('node:test');

test('Cargo, Tauri, npm and both lock root versions agree', () => {
  const pkg = JSON.parse(fs.readFileSync('package.json', 'utf8'));
  const lock = JSON.parse(fs.readFileSync('package-lock.json', 'utf8'));
  const tauri = JSON.parse(fs.readFileSync('tauri.conf.json', 'utf8'));
  const cargo = fs.readFileSync('Cargo.toml', 'utf8').match(/^version = "([^"]+)"/m)[1];
  const cargoLock = fs.readFileSync('Cargo.lock', 'utf8').match(/name = "bald"\s+version = "([^"]+)"/)[1];
  assert.deepEqual([lock.version, lock.packages[''].version, tauri.version, cargo, cargoLock], Array(5).fill(pkg.version));
});

test('version bump writes both npm lock versions before staging, with all external commands mocked', () => {
  const files = new Map();
  for (const name of ['package.json', 'package-lock.json', 'tauri.conf.json', 'Cargo.toml']) {
    files.set(path.join('/fixture', name), fs.readFileSync(name, 'utf8'));
  }
  const calls = [];
  const fakeFs = { readFileSync: name => files.get(name), writeFileSync: (name, value) => files.set(name, value) };
  vm.runInNewContext(fs.readFileSync('bump-and-push.js', 'utf8'), {
    __dirname: '/fixture', console: { log() {}, error() {} }, process: { exit() { throw Error('unexpected exit'); } },
    require: name => name === 'fs' ? fakeFs : name === 'path' ? path : {
      execFileSync: (program, args) => { calls.push([program, args]); return ''; }
    }
  });
  const pkg = JSON.parse(files.get(path.join('/fixture', 'package.json')));
  const lock = JSON.parse(files.get(path.join('/fixture', 'package-lock.json')));
  assert.equal(lock.version, pkg.version);
  assert.equal(lock.packages[''].version, pkg.version);
  assert.ok(calls.some(([program, args]) => program === 'git' && args[0] === 'add' && args.includes('package-lock.json')));
});
