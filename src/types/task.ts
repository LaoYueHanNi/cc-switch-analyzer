// 任务相关类型（与后端 Task / TaskSession / TaskWithStats / TaskDetail 对应）

export type TaskStatus = 'todo' | 'in_progress' | 'done' | 'archived'

export const TASK_STATUS_OPTIONS: { value: TaskStatus; label: string; color: string }[] = [
  { value: 'todo', label: '未开始', color: 'default' },
  { value: 'in_progress', label: '进行中', color: 'info' },
  { value: 'done', label: '已完成', color: 'success' },
  { value: 'archived', label: '已废弃', color: 'warning' }
]

export interface Task {
  id: number
  title: string
  description: string
  status: TaskStatus
  createdAt: number
  updatedAt: number
}

export interface TaskSession {
  taskId: number
  sessionId: string
  source: string
  projectDir: string
  title: string
  addedAt: number
}

export interface TaskSessionInput {
  sessionId: string
  source: string
  projectDir?: string
  title?: string
}

export interface TaskWithStats {
  // flatten Task 字段
  id: number
  title: string
  description: string
  status: TaskStatus
  createdAt: number
  updatedAt: number
  sessionCount: number
  totalTokens: number
  totalCost: number
  sessions: TaskSession[]
}

export interface TaskDetail {
  id: number
  title: string
  description: string
  status: TaskStatus
  createdAt: number
  updatedAt: number
  sessionCount: number
  totalTokens: number
  totalCost: number
  sessions: TaskSession[]
}
