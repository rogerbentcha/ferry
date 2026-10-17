import { describe, expect, it } from 'vitest'

import { filterLogLines, lineSeverity, logLineText, logSeverity, type LogViewerLine } from './logs'

describe('logSeverity', () => {
  it('keeps normal stderr progress neutral', () => {
    expect(logSeverity('stderr', '#5 [2/4] RUN npm ci')).toBeNull()
    expect(logSeverity('stderr', '#8 DONE 0.4s')).toBeNull()
    expect(logSeverity('stderr', '172.18.0.1 - - [26/Sep/2026:20:44:57 +0000] "GET / HTTP/1.1" 200 615')).toBeNull()
    expect(logSeverity('stderr', 'INFO:     Application startup complete.')).toBeNull()
  })

  it('flags error-level lines on any stream', () => {
    expect(logSeverity('stderr', '#9 ERROR: failed to solve: process "/bin/sh -c npm ci" did not complete')).toBe('error')
    expect(logSeverity('stderr', '2026/09/26 20:44:57 [error] 29#29: *1 open() failed')).toBe('error')
    expect(logSeverity('stdout', 'Traceback (most recent call last):')).toBe('error')
    expect(logSeverity('stdout', '{"level":"error","msg":"db down"}')).toBe('error')
    expect(logSeverity('stdout', 'time=… level=fatal msg=boom')).toBe('error')
    expect(logSeverity('stdout', "panic: runtime error: index out of range")).toBe('error')
    expect(logSeverity('stdout', 'ERROR: relation "users" does not exist')).toBe('error')
  })

  it('does not match words that merely contain a severity', () => {
    expect(logSeverity('stdout', 'compiled with 0 errors')).toBeNull()
    expect(logSeverity('stdout', 'error_log /var/log/nginx/error.log')).toBeNull()
    expect(logSeverity('stdout', 'terror')).toBeNull()
  })

  it('flags warnings', () => {
    expect(logSeverity('stderr', 'npm WARN deprecated inflight@1.0.6')).toBe('warning')
    expect(logSeverity('stdout', '[warning] disk almost full')).toBe('warning')
  })

  it('only marks failing system lines', () => {
    expect(logSeverity('system', '==> Starting deploy dep-1 (trigger: create)')).toBeNull()
    expect(logSeverity('system', '==> Deploy failed: instance d84d6c crashed (exit code 1)')).toBe('error')
  })
})

describe('lineSeverity', () => {
  const line = (more: Partial<LogViewerLine>): LogViewerLine => ({ seq: 1, ts: '2026-10-17T12:00:00Z', text: '', ...more })

  it('follows the level of a line of the server log, whatever its text', () => {
    expect(lineSeverity(line({ level: 'error', text: 'proxy stopped' }))).toBe('error')
    expect(lineSeverity(line({ level: 'warn', text: 'no certificate' }))).toBe('warning')
    expect(lineSeverity(line({ level: 'info', text: 'request failed with error 500' }))).toBeNull()
    expect(lineSeverity(line({ level: 'debug', text: 'warning: …' }))).toBeNull()
  })

  it('reads the text of other lines, and gives a marker none', () => {
    expect(lineSeverity(line({ stream: 'stderr', text: 'npm WARN deprecated' }))).toBe('warning')
    expect(lineSeverity(line({ text: 'ERROR: boom' }))).toBe('error')
    expect(lineSeverity(line({ skipped: 3, text: '3 lines skipped' }))).toBeNull()
  })
})

describe('filterLogLines', () => {
  const lines: LogViewerLine[] = [
    { seq: 1, ts: 't', text: 'connected a GitHub account', level: 'info' },
    { seq: 2, ts: 't', text: 'certificate issued', level: 'info' },
    { seq: 3, ts: 't', text: '20 lines skipped', skipped: 20 },
    { seq: 4, ts: 't', text: 'push from github.com/acme/web', level: 'info' },
    { seq: 5, ts: 't', text: 'listening', instance: 'a1b2c3' },
  ]

  it('keeps the lines that mention the search, case aside, and the markers', () => {
    expect(filterLogLines(lines, 'github').map((l) => l.seq)).toEqual([1, 3, 4])
    expect(filterLogLines(lines, 'a1b2').map((l) => l.seq)).toEqual([3, 5])
    expect(filterLogLines(lines, '')).toBe(lines)
  })
})

describe('logLineText', () => {
  it('is the line as one line of text', () => {
    expect(logLineText({ seq: 1, ts: '2026-10-17T12:00:00Z', text: 'no certificate host=a.b', level: 'warn' })).toBe(
      '2026-10-17T12:00:00Z WARN no certificate host=a.b',
    )
    expect(logLineText({ seq: 1, ts: '2026-10-17T12:00:00Z', text: 'hi', stream: 'stdout', instance: 'a1b2c3' })).toBe(
      '2026-10-17T12:00:00Z [a1b2c3] hi',
    )
  })
})
