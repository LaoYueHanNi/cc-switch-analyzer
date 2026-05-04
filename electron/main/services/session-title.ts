import { readdirSync, readFileSync, existsSync } from 'fs'
import { homedir } from 'os'
import { join } from 'path'
import type { AppDbService } from './app-db'

function claudeProjectsDir(): string {
  return join(homedir(), '.claude', 'projects')
}

function shortSession(sessionId: string): string {
  return sessionId.split('-')[0] || sessionId.slice(0, 8)
}

function findJsonl(sessionId: string): { path: string; project: string } | null {
  const dir = claudeProjectsDir()
  if (!existsSync(dir)) return null
  const target = `${sessionId}.jsonl`
  try {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      if (entry.isDirectory()) {
        const candidate = join(dir, entry.name, target)
        if (existsSync(candidate)) return { path: candidate, project: entry.name }
      }
    }
  } catch { /* ignore */ }
  return null
}

function extractFirstUserMessage(path: string): string | null {
  try {
    const content = readFileSync(path, 'utf-8')
    for (const line of content.split('\n')) {
      if (!line.trim()) continue
      try {
        const obj = JSON.parse(line)
        if (obj.type !== 'user') continue
        const c = obj.message?.content
        if (!c) continue
        if (typeof c === 'string' && c.trim()) return c
        if (Array.isArray(c)) {
          const text = c.filter((b: any) => b.type === 'text' && b.text?.trim()).map((b: any) => b.text).join(' ')
          if (text.trim()) return text
        }
      } catch { /* skip */ }
    }
  } catch { /* ignore */ }
  return null
}

function cleanTitle(text: string): string {
  return text.trim().replace(/^\/+/, '').split('\n')[0]?.trim() || ''
}

export function resolveSessionTitles(
  appDb: AppDbService,
  sessionIds: string[]
): Map<string, { title: string; project: string }> {
  const result = new Map<string, { title: string; project: string }>()
  if (sessionIds.length === 0) return result

  const cached = appDb.getSessionTitles(sessionIds)
  for (const [sid, raw] of cached) {
    const [title, ...rest] = raw.split('|')
    result.set(sid, { title, project: rest.join('|') })
  }

  const uncached = sessionIds.filter(id => !cached.has(id))
  if (uncached.length === 0) return result

  for (const id of uncached) {
    const found = findJsonl(id)
    const title = found ? (extractFirstUserMessage(found.path) ? cleanTitle(extractFirstUserMessage(found.path)!) : shortSession(id)) : shortSession(id)
    const project = found?.project || ''
    appDb.saveSessionTitle(id, `${title}|${project}`)
    result.set(id, { title, project })
  }
  return result
}
