// 从 package.json 读取版本号，同步到 Cargo.toml 和 tauri.conf.json
import fs from 'fs'
import path from 'path'
import { fileURLToPath } from 'url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const pkg = JSON.parse(fs.readFileSync(path.join(__dirname, 'package.json'), 'utf8'))
const version = process.argv[2] || pkg.version

const replaceInFile = (file, pattern, replacement) => {
  const p = path.join(__dirname, file)
  const content = fs.readFileSync(p, 'utf8')
  const newContent = content.replace(pattern, replacement)
  if (content !== newContent) {
    fs.writeFileSync(p, newContent)
    console.log(`  ${file} -> ${version}`)
  }
}

console.log(`Syncing version: ${version}`)
replaceInFile('src-tauri/Cargo.toml', /^version\s*=\s*"[^"]+"/m, `version = "${version}"`)
replaceInFile('src-tauri/tauri.conf.json', /"version":\s*"[^"]+"/, `"version": "${version}"`)