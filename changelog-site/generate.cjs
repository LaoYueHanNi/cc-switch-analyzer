#!/usr/bin/env node
/**
 * 从 git log 生成 changelog 数据 (JSON)
 * 用法: node generate.js > data.json
 */
const { execSync } = require('child_process')

const rawLog = execSync(
  'git log --format="%H|%s|%ai" --no-merges',
  { encoding: 'utf-8', cwd: __dirname + '/..' }
).trim()

if (!rawLog) {
  console.log('[]')
  process.exit(0)
}

const commits = rawLog.split('\n').map(line => {
  const [hash, subject, date] = line.split('|')
  return { hash: hash.slice(0, 7), subject, date: date.slice(0, 10) }
})

// 版本号提交匹配
const VERSION_RE = /^chore: 版本号 .+ → (.+)$/
const versionIndexes = []

commits.forEach((c, i) => {
  const m = c.subject.match(VERSION_RE)
  if (m) {
    versionIndexes.push({ index: i, version: m[1], date: c.date })
  }
})

// 未发布部分（最新版本号之前的提交）
const releases = []
const firstVersionIdx = versionIndexes.length > 0 ? versionIndexes[0].index : commits.length

if (firstVersionIdx > 0) {
  releases.push({
    version: 'Unreleased',
    date: commits[0].date,
    commits: commits.slice(0, firstVersionIdx).map(c => ({
      hash: c.hash,
      subject: c.subject,
      date: c.date
    }))
  })
}

// 各版本
for (let i = 0; i < versionIndexes.length; i++) {
  const v = versionIndexes[i]
  const nextIdx = i + 1 < versionIndexes.length ? versionIndexes[i + 1].index : commits.length
  const versionCommits = commits.slice(v.index + 1, nextIdx)

  releases.push({
    version: v.version,
    date: v.date,
    commits: versionCommits.map(c => ({
      hash: c.hash,
      subject: c.subject,
      date: c.date
    }))
  })
}

console.log(JSON.stringify(releases, null, 2))
