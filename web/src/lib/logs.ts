/**
 * Severity of one log line, for colouring. The stream alone is not enough:
 * Docker BuildKit, nginx and Python write normal progress to stderr, so a red
 * stderr would make healthy output look like failures. Only lines that say
 * they are errors / warnings get the destructive / warning ink. A line of the
 * server's own log says it: its level decides.
 */

import type { LogStreamKind, ServerLogLevel } from './api/types'

export type LogSeverity = 'error' | 'warning' | null

/**
 * A line of the log viewer: a runtime, build or job line (`LogEntry`), a line
 * of the server's own log (it has a `level`), or a marker that stands for
 * lines the stream skipped.
 */
export interface LogViewerLine {
  /** Stable sequence number (the row's key). */
  seq: number
  ts: string
  /** The text without ANSI codes: what the viewer renders and searches. */
  text: string
  stream?: LogStreamKind
  /** Short container id (runtime logs). */
  instance?: string | null
  /** A line of the server's log: its level. */
  level?: ServerLogLevel
  /** A line of the server's log: the module that logged it. */
  target?: string
  /** A marker: this many lines were skipped here. */
  skipped?: number
}

// A severity word standing alone (not "errors", "error_log", "/error.log"…).
const WORD_BOUNDARY_BEFORE = String.raw`(?:^|[\s\[(<:"'|=-])`
const WORD_BOUNDARY_AFTER = String.raw`(?=$|[\s\]):>"'|,!-])`

const ERROR_RE = new RegExp(
  `${WORD_BOUNDARY_BEFORE}(?:error|err|fatal|panic|critical|crit|emerg|alert|severe|exception)${WORD_BOUNDARY_AFTER}` +
    String.raw`|^Traceback \(most recent call last\)|^panic:|"level"\s*:\s*"(?:error|fatal|panic|critical)"|\blevel=(?:error|fatal|panic|critical)\b`,
  'i',
)
const WARNING_RE = new RegExp(
  `${WORD_BOUNDARY_BEFORE}(?:warn|warning|deprecated)${WORD_BOUNDARY_AFTER}` +
    String.raw`|"level"\s*:\s*"warn(?:ing)?"|\blevel=warn(?:ing)?\b`,
  'i',
)
// Ferry's own `==>` lines: only failures are errors.
const SYSTEM_ERROR_RE = /\b(?:failed|failure|error|crashed|timed out|unhealthy)\b/i

export function logSeverity(stream: string, text: string): LogSeverity {
  if (stream === 'system') return SYSTEM_ERROR_RE.test(text) ? 'error' : null
  if (ERROR_RE.test(text)) return 'error'
  if (WARNING_RE.test(text)) return 'warning'
  return null
}

const LEVEL_SEVERITY: Record<ServerLogLevel, LogSeverity> = { error: 'error', warn: 'warning', info: null, debug: null }

/** Severity of a line of the viewer: from its level when it has one, else from its text. */
export function lineSeverity(line: LogViewerLine): LogSeverity {
  if (line.skipped !== undefined) return null
  return line.level ? LEVEL_SEVERITY[line.level] : logSeverity(line.stream ?? 'stdout', line.text)
}

/**
 * The lines that contain `needle` (lowercase; empty keeps them all), and the
 * skipped-lines markers, which say where lines may be missing.
 */
export function filterLogLines<T extends LogViewerLine>(lines: T[], needle: string): T[] {
  if (!needle) return lines
  return lines.filter(
    (l) => l.skipped !== undefined || l.text.toLowerCase().includes(needle) || (l.instance ?? '').includes(needle),
  )
}

/** A line as one line of text, as its copy button copies it. */
export function logLineText(line: LogViewerLine): string {
  return [line.ts, line.level?.toUpperCase(), line.instance ? `[${line.instance}]` : null, line.text]
    .filter(Boolean)
    .join(' ')
}
