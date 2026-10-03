const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

function git(...args) {
  console.log(`> git ${args.join(' ')}`);
  return execFileSync('git', args, { cwd: __dirname, encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'] }).trim();
}

if (git('status', '--porcelain')) {
  console.error('릴리스 전에 작업 트리를 커밋해 주세요. 변경 사항이 남아 있어 중단합니다.');
  process.exit(1);
}
git('pull', '--rebase', 'origin', 'main');

const packagePath = path.join(__dirname, 'package.json');
const tauriPath = path.join(__dirname, 'tauri.conf.json');
const cargoPath = path.join(__dirname, 'Cargo.toml');
const pkg = JSON.parse(fs.readFileSync(packagePath, 'utf8'));
const tauri = JSON.parse(fs.readFileSync(tauriPath, 'utf8'));
const parts = pkg.version.split('.').map(Number);
parts[2] += 1;
const version = parts.join('.');

pkg.version = version;
tauri.version = version;
fs.writeFileSync(packagePath, `${JSON.stringify(pkg, null, 2)}\n`);
fs.writeFileSync(tauriPath, `${JSON.stringify(tauri, null, 2)}\n`);
const cargo = fs.readFileSync(cargoPath, 'utf8').replace(/^version = "[^"]+"/m, `version = "${version}"`);
fs.writeFileSync(cargoPath, cargo);

execFileSync('cargo', ['check'], { cwd: __dirname, stdio: 'inherit' });
git('add', 'package.json', 'package-lock.json', 'tauri.conf.json', 'Cargo.toml', 'Cargo.lock');
git('commit', '-m', `chore: bump version to v${version}`);
git('tag', `v${version}`);
git('push', 'origin', 'main');
git('push', 'origin', `v${version}`);
console.log(`Bald v${version} 릴리스를 시작했습니다.`);
