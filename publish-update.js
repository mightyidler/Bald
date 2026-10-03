const fs = require('fs');
const path = require('path');
const https = require('https');

const owner = 'mightyidler';
const repo = 'Bald';
const token = process.env.GITHUB_TOKEN;
if (!token) throw new Error('GITHUB_TOKEN이 필요합니다.');

const version = JSON.parse(fs.readFileSync(path.join(__dirname, 'tauri.conf.json'), 'utf8')).version;
const tag = `v${version}`;

function request(url, headers = {}) {
  return new Promise((resolve, reject) => {
    https.get(url, { headers }, response => {
      if (response.statusCode >= 300 && response.statusCode < 400 && response.headers.location) {
        response.resume();
        return request(response.headers.location, headers).then(resolve, reject);
      }
      let body = '';
      response.on('data', chunk => body += chunk);
      response.on('end', () => response.statusCode >= 200 && response.statusCode < 300
        ? resolve(body)
        : reject(new Error(`HTTP ${response.statusCode}: ${body}`)));
    }).on('error', reject);
  });
}

async function main() {
  const headers = {
    'User-Agent': 'Bald-Updater',
    Authorization: `Bearer ${token}`,
    Accept: 'application/vnd.github+json',
    'X-GitHub-Api-Version': '2022-11-28',
  };
  let release;
  for (let attempt = 1; attempt <= 6; attempt += 1) {
    try {
      release = JSON.parse(await request(`https://api.github.com/repos/${owner}/${repo}/releases/tags/${tag}`, headers));
      break;
    } catch (error) {
      if (attempt === 6) throw error;
      await new Promise(resolve => setTimeout(resolve, 5000));
    }
  }
  const installer = release.assets.find(asset => asset.name.endsWith('-setup.exe'));
  const signatureAsset = release.assets.find(asset => asset.name.endsWith('-setup.exe.sig'));
  if (!installer || !signatureAsset) throw new Error('NSIS 설치 파일 또는 서명 파일을 찾지 못했습니다.');
  const signature = (await request(signatureAsset.browser_download_url, { 'User-Agent': 'Bald-Updater' })).trim();
  const manifest = {
    version,
    notes: `Bald ${version} 업데이트`,
    pub_date: new Date().toISOString(),
    platforms: {
      'windows-x86_64': { signature, url: installer.browser_download_url },
    },
  };
  fs.writeFileSync(path.join(__dirname, 'update.json'), `${JSON.stringify(manifest, null, 2)}\n`);
  console.log(`update.json을 Bald ${version} 정보로 갱신했습니다.`);
}

main().catch(error => {
  console.error(error.message);
  process.exit(1);
});
