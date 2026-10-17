/**
 * The server's own log (`GET /api/v1/server/logs`, DESIGN.md §22) as lines of
 * the log viewer: each `event: log` with its level, and an `event: skipped`
 * marker where this page fell behind the server and missed lines.
 */

import type { SseEvent } from './api/sse'
import type { ServerLogLevel, ServerLogLine } from './api/types'
import { stripAnsi } from './format'
import type { LogViewerLine } from './logs'

/** Lines the server keeps (and the page). */
export const SERVER_LOG_LINES = 2000

/** The levels of the filter, the most important first. */
export const SERVER_LOG_LEVELS: readonly ServerLogLevel[] = ['error', 'warn', 'info', 'debug']

/** A line as ferryd prints it: the message, then `key=value` for each field. */
export function serverLogText(line: ServerLogLine): string {
  const fields = Object.entries(line.fields ?? {}).map(([key, value]) => `${key}=${value}`)
  return stripAnsi([line.message, ...fields].join(' '))
}

/** The text of a skipped-lines marker. */
export function skippedText(count: number): string {
  const lines = `${count.toLocaleString('en-US')} line${count === 1 ? '' : 's'}`
  return `${lines} skipped: this page fell behind the server`
}

/** One event of the stream as a line of the viewer; null leaves it out. */
export function parseServerLogEvent(event: SseEvent, seq: number): LogViewerLine | null {
  try {
    if (event.event === 'log') {
      const line = JSON.parse(event.data) as ServerLogLine
      if (typeof line.message !== 'string' || !SERVER_LOG_LEVELS.includes(line.level)) return null
      return { seq, ts: line.ts, level: line.level, target: line.target, text: serverLogText(line) }
    }
    if (event.event === 'skipped') {
      const { count } = JSON.parse(event.data) as { count?: unknown }
      if (typeof count !== 'number') return null
      return { seq, ts: new Date().toISOString(), skipped: count, text: skippedText(count) }
    }
  } catch {
    /* malformed: skip */
  }
  return null
}
