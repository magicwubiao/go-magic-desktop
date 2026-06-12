// 版本管理工具：以 package.json 为唯一版本源，同步到 Cargo.toml、tauri.conf.json 和 VERSION.txt
// 用法:
//   node sync-version.mjs              同步所有文件
//   node sync-version.mjs 1.2.3        设置具体版本
//   node sync-version.mjs --patch      递增补丁版本 (0.4.12 → 0.4.13)
//   node sync-version.mjs --minor      递增次版本号 (0.4.12 → 0.5.0)
//   node sync-version.mjs --major      递增主版本号 (0.4.12 → 1.0.0)
import fs from 'fs'
import path from 'path'
import { fileURLToPath } from 'url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const pkgPath = path.join(__dirname, 'package.json')
const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8'))

const validateVersion = (v) => {
  const semver = /^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$/
  if (!semver.test(v)) {
    throw new Error(`Invalid version: "${v}". Must follow SemVer (e.g. 1.2.3, 1.2.3-beta.1)`)
  }
  return v
}

const bump = (current, type) => {
  const [major, minor, patch] = current.split('.').map(n => parseInt(n, 10))
  if (type === 'major') return `${major + 1}.0.0`
  if (type === 'minor') return `${major}.${minor + 1}.0`
  if (type === 'patch') return `${major}.${minor}.${patch + 1}`
  return current
}

const arg = process.argv[2]
let targetVersion = pkg.version
let action = 'sync'

if (arg === '--patch' || arg === '--minor' || arg === '--major') {
  const type = arg.slice(2)
  targetVersion = validateVersion(bump(pkg.version, type))
  action = `bump-${type}`
} else if (arg && !arg.startsWith('-')) {
  targetVersion = validateVersion(arg)
  action = 'set'
}

const replaceInFile = (file, pattern, replacement) => {
  const p = path.join(__dirname, file)
  if (!fs.existsSync(p)) {
    console.warn(`  ⚠  Skip missing file: ${file}`)
    return
  }
  const content = fs.readFileSync(p, 'utf8')
  const newContent = content.replace(pattern, replacement)
  if (content !== newContent) {
    fs.writeFileSync(p, newContent)
    console.log(`  ✓ ${file} -> ${targetVersion}`)
  } else {
    console.log(`  = ${file} already in sync`)
  }
}

console.log(`\n🔄 ${action === 'sync' ? 'Syncing' : `Bumping to`} version: ${targetVersion}`)

// 1. 更新 package.json（如果需要改版本号）
if (action !== 'sync') {
  pkg.version = targetVersion
  fs.writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + '\n')
  console.log(`  ✓ package.json -> ${targetVersion}`)
}

// 2. 同步到 Cargo.toml
replaceInFile('src-tauri/Cargo.toml', /^version\s*=\s*"[^"]+"/m, `version = "${targetVersion}"`)

// 3. 同步到 tauri.conf.json
replaceInFile('src-tauri/tauri.conf.json', /"version":\s*"[^"]+"/, `"version": "${targetVersion}"`)

// 4. 更新 VERSION.txt（自动更新打包日期）
const now = new Date()
const timestamp = now.toISOString().replace('T', ' ').replace(/\.\d+Z$/, ' UTC')
const versionTxt = `Go Magic Desktop Source Package
Version: ${targetVersion}
Packaged: ${timestamp}

Contents:
- go-magic-desktop/ : Desktop application source (Tauri + Web)

For build instructions, see:
- README.md
`
fs.writeFileSync(path.join(__dirname, 'VERSION.txt'), versionTxt)
console.log(`  ✓ VERSION.txt -> ${targetVersion} (${timestamp})`)

// 5. 一致性检查
console.log(`\n🔍 Verifying consistency...`)
const checkFile = (file, extract) => {
  const p = path.join(__dirname, file)
  if (!fs.existsSync(p)) return true
  const content = fs.readFileSync(p, 'utf8')
  const found = extract(content)
  const ok = found === targetVersion
  console.log(`  ${ok ? '✓' : '✗'} ${file}: ${found || '(not found)'}${ok ? '' : ' (expected ' + targetVersion + ')'}`)
  return ok
}

const allOk = [
  checkFile('package.json', c => JSON.parse(c).version),
  checkFile('src-tauri/Cargo.toml', c => (c.match(/^version\s*=\s*"([^"]+)"/m) || [])[1]),
  checkFile('src-tauri/tauri.conf.json', c => {
    try { return JSON.parse(c).version } catch { return (c.match(/"version":\s*"([^"]+)"/) || [])[1] }
  }),
  checkFile('VERSION.txt', c => (c.match(/^Version:\s*(\S+)/m) || [])[1]),
].every(Boolean)

console.log(allOk
  ? `\n✅ All files in sync. Version: ${targetVersion}\n`
  : `\n❌ Version mismatch detected! Check the files above.\n`
)

if (!allOk) process.exit(1)