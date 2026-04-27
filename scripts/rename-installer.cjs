// NSIS 설치 파일 이름을 'TaskMon-Setup-v{버전}.exe' 형식으로 변경하는 빌드 후 스크립트.
// Tauri NSIS 기본 출력명: TaskMon_{버전}_{아키텍처}-setup.exe
// 변경 후 이름:           TaskMon-Setup-v{버전}.exe

const fs = require('fs');
const path = require('path');

// tauri.conf.json에서 버전 읽기
const configPath = path.join(__dirname, '..', 'src-tauri', 'tauri.conf.json');
let version;
try {
  const config = JSON.parse(fs.readFileSync(configPath, 'utf-8'));
  version = config.version;
} catch (err) {
  console.error(`[ERROR] tauri.conf.json 읽기 실패: ${err.message}`);
  process.exit(1);
}

if (!version) {
  console.error('[ERROR] tauri.conf.json에 version 필드가 없습니다.');
  process.exit(1);
}

const bundleDir = path.join(__dirname, '..', 'src-tauri', 'target', 'release', 'bundle', 'nsis');
const newName = `TaskMon-Setup-v${version}.exe`;
const newPath = path.join(bundleDir, newName);

// NSIS 기본 출력 패턴(아키텍처는 x64/x86/arm64 가능)을 와일드카드로 탐색
let candidates = [];
try {
  candidates = fs
    .readdirSync(bundleDir)
    .filter((f) => /^TaskMon_.+-setup\.exe$/i.test(f) && f !== newName);
} catch (err) {
  console.error(`[ERROR] NSIS 출력 디렉터리를 읽을 수 없습니다: ${bundleDir}`);
  console.error(`        원인: ${err.message}`);
  process.exit(1);
}

if (candidates.length === 0) {
  console.error(`[ERROR] NSIS 설치 파일을 찾지 못했습니다: ${bundleDir}`);
  process.exit(1);
}

// 여러 개 후보가 있으면(드문 경우) 최신 수정 시간 기준 선택
const oldName = candidates
  .map((f) => ({ f, mtime: fs.statSync(path.join(bundleDir, f)).mtimeMs }))
  .sort((a, b) => b.mtime - a.mtime)[0].f;
const oldPath = path.join(bundleDir, oldName);

// 동일 이름의 이전 결과물이 있으면 덮어쓰기
if (fs.existsSync(newPath)) {
  fs.unlinkSync(newPath);
}
fs.renameSync(oldPath, newPath);
console.log(`[OK] 설치 파일 이름 변경: ${oldName} -> ${newName}`);
