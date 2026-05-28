// CI 脚本：生成 latest.json（Tauri v2 updater 清单）
const fs = require('fs')
const path = require('path')
const { execSync } = require('child_process')

const TAG = process.env.TAG
if (!TAG) { console.error('Missing TAG env var'); process.exit(1) }

const VERSION = TAG.replace(/^v/, '')
const NOTES = execSync('git log -1 --format=%b HEAD', { encoding: 'utf-8' }).trim()
const PUB_DATE = new Date().toISOString()

const PLATFORM_BASE = `https://github.com/LaoYueHanNi/cc-switch-analyzer/releases/download/${TAG}`

const platforms = {}

// Windows MSI
const msiDir = path.join('artifacts', 'windows-msi')
if (fs.existsSync(msiDir)) {
  const msiFiles = fs.readdirSync(msiDir).filter(f => f.endsWith('.msi'))
  if (msiFiles.length > 0) {
    const msiName = msiFiles[0]
    const sigName = msiName + '.sig'
    const sigPath = path.join(msiDir, sigName)
    const signature = fs.existsSync(sigPath) ? fs.readFileSync(sigPath, 'utf-8').trim() : ''
    const size = fs.statSync(path.join(msiDir, msiName)).size
    const url = `${PLATFORM_BASE}/${msiName.replace(/ /g, '.')}`
    console.log(`Windows MSI: ${msiName} (${(size / 1024 / 1024).toFixed(1)} MB)`)
    console.log(`  URL: ${url}`)
    console.log(`  Signature: ${signature ? 'OK' : 'MISSING'}`)
    platforms['windows-x86_64'] = { url, size, signature }
  }
}

// macOS
const dmgDir = path.join('artifacts', 'macos-dmg')
if (fs.existsSync(dmgDir)) {
  // .tar.gz for updater (Tauri v2 uses tar.gz on macOS, not dmg)
  const tarFiles = fs.readdirSync(dmgDir).filter(f => f.endsWith('.tar.gz'))
  for (const file of tarFiles) {
    const filePath = path.join(dmgDir, file)
    const sigPath = filePath + '.sig'
    const signature = fs.existsSync(sigPath) ? fs.readFileSync(sigPath, 'utf-8').trim() : ''
    const size = fs.statSync(filePath).size
    const url = `${PLATFORM_BASE}/${file.replace(/ /g, '.')}`
    const platformKey = (file.includes('x86_64') || file.includes('x64')) ? 'darwin-x86_64' : 'darwin-aarch64'
    console.log(`macOS ${platformKey}: ${file} (${(size / 1024 / 1024).toFixed(1)} MB)`)
    console.log(`  URL: ${url}`)
    console.log(`  Signature: ${signature ? 'OK' : 'MISSING'}`)
    platforms[platformKey] = { url, size, signature }
  }
}

const manifest = {
  version: VERSION,
  notes: NOTES,
  pub_date: PUB_DATE,
  platforms,
}

fs.writeFileSync('latest.json', JSON.stringify(manifest, null, 2))
console.log('\nGenerated latest.json:')
console.log(JSON.stringify(manifest, null, 2))
