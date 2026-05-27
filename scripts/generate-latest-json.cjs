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
  const sigFiles = fs.readdirSync(msiDir).filter(f => f.endsWith('.msi.sig'))
  if (msiFiles.length > 0) {
    const msiName = msiFiles[0]
    const sigName = sigFiles[0] || msiName + '.sig'
    const sigPath = path.join(msiDir, sigName)
    const signature = fs.existsSync(sigPath) ? fs.readFileSync(sigPath, 'utf-8').trim() : ''
    const size = fs.statSync(path.join(msiDir, msiName)).size
    platforms['windows-x86_64'] = { url: `${PLATFORM_BASE}/${msiName}`, size, signature }
  }
}

// macOS DMG
const dmgDir = path.join('artifacts', 'macos-dmg')
if (fs.existsSync(dmgDir)) {
  const dmgFiles = fs.readdirSync(dmgDir).filter(f => f.endsWith('.dmg'))
  // Also check for .tar.gz (Tauri updater on macOS uses tar.gz, not dmg)
  const tarFiles = fs.readdirSync(dmgDir).filter(f => f.endsWith('.tar.gz'))
  const allFiles = dmgFiles.concat(tarFiles)

  for (const file of allFiles) {
    const filePath = path.join(dmgDir, file)
    const sigPath = filePath + '.sig'
    const signature = fs.existsSync(sigPath) ? fs.readFileSync(sigPath, 'utf-8').trim() : ''
    const size = fs.statSync(filePath).size

    let platformKey = 'darwin-aarch64'
    if (file.includes('x86_64') || file.includes('x64')) {
      platformKey = 'darwin-x86_64'
    }

    platforms[platformKey] = { url: `${PLATFORM_BASE}/${file}`, size, signature }
  }
}

const manifest = {
  version: VERSION,
  notes: NOTES,
  pub_date: PUB_DATE,
  platforms,
}

fs.writeFileSync('latest.json', JSON.stringify(manifest, null, 2))
console.log('Generated latest.json:')
console.log(JSON.stringify(manifest, null, 2))
