const fs = require('fs');

const path = require('path');

const { spawnSync } = require('child_process');

const keyPath = path.join(__dirname, 'updater.key');

if (!fs.existsSync(keyPath)) {
  console.error('[build-signed] updater.key 파일이 없습니다. GitHub Actions에서는 signing secret을 사용하세요.');
  process.exit(1);
}

const result = spawnSync('npx', ['tauri', 'build'], {
  cwd: __dirname,
  stdio: 'inherit',
  shell: process.platform === 'win32',
  env: { ...process.env, TAURI_SIGNING_PRIVATE_KEY: fs.readFileSync(keyPath, 'utf8').trim() },
});

process.exit(result.status ?? 1);
