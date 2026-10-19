import { useCallback, useEffect, useRef, useState } from 'react'

import { stripAnsi } from '@/lib/format'
import { useSignedIn } from '@/stores/auth'

import { ApiError, errorMessage } from './client'
import { backoffDelay, isAbortError, readSse, sleep, type SseEvent } from './sse'
import type { LogLine } from './types'

export type LogStreamStatus = 'idle' | 'connecting' | 'live' | 'ended' | 'error'

/** A received line plus a stable sequence number and its ANSI-free text. */
export interface LogEntry extends LogLine {
  seq: number
  /** `line` without ANSI escape codes (what the viewer renders / searches). */
  text: string
}

/** One event of the stream as a line (`seq` is its number), or null to leave it out. */
export type LogEventParser<T> = (event: SseEvent, seq: number) => T | null

export interface LogStreamOptions<T = LogEntry> {
  /** Keep the stream open for new lines (`?follow=true`). Default true. */
  follow?: boolean
  /** Runtime logs only: number of past lines per instance (`?tail=`). */
  tail?: number
  /** Other query parameters (e.g. the server log's `level`). */
  query?: Record<string, string | number | boolean>
  /**
   * Turns events into lines; a module-level function (a new one reopens the
   * stream). Default: `event: log` with a `LogLine`.
   */
  parse?: LogEventParser<T>
  /** Set false to keep the stream closed. Default true. */
  enabled?: boolean
  /** Ring-buffer size; older lines are dropped. Default 5000. */
  maxLines?: number
  /** Reconnect after network drops while following. Default true. */
  reconnect?: boolean
}

export interface LogStreamResult<T = LogEntry> {
  lines: T[]
  status: LogStreamStatus
  /** Error message when status is `error` (or the last reconnect reason). */
  error: string | null
  /** Drop the lines received so far (the stream stays open). */
  clear: () => void
  /** Close and reopen the stream from scratch. */
  restart: () => void
}

/** `event: log` with a `LogLine` (runtime, build and job logs). */
export function parseLogLine(event: SseEvent, seq: number): LogEntry | null {
  if (event.event !== 'log') return null
  try {
    const line = JSON.parse(event.data) as LogLine
    if (typeof line.line !== 'string') return null
    return { ...line, seq, text: stripAnsi(line.line) }
  } catch {
    return null // malformed line: skip
  }
}

/**
 * Stream an SSE log endpoint (`event: log` lines, `event: end` terminator).
 *
 * `path` e.g. `streamPaths.deployLogs(id)`; pass null to stay idle. A query
 * in `path` (the `?environment=` of `streamPaths.serviceLogs`) is kept.
 * Lines are batched per animation frame so fast builds don't re-render per
 * line. On a network drop while following it reconnects with backoff:
 * finite logs (deploy/job) and the server's log are replayed from scratch,
 * runtime logs (with `tail`) resume with `tail=0` so nothing is duplicated.
 */
export function useLogStream(path: string | null, options?: LogStreamOptions<LogEntry>): LogStreamResult<LogEntry>
export function useLogStream<T extends { seq: number }>(
  path: string | null,
  options: LogStreamOptions<T> & { parse: LogEventParser<T> },
): LogStreamResult<T>
export function useLogStream<T extends { seq: number }>(
  path: string | null,
  options: LogStreamOptions<T> = {},
): LogStreamResult<T> {
  const { follow = true, tail, query, enabled = true, maxLines = 5000, reconnect = true } = options
  // Without `parse`, the first signature: T is LogEntry.
  const parse = options.parse ?? (parseLogLine as unknown as LogEventParser<T>)
  const signedIn = useSignedIn()
  // The query as a value, so a new object with the same parameters keeps the stream.
  const queryKey = query ? JSON.stringify(query) : ''

  const [lines, setLines] = useState<T[]>([])
  const [status, setStatus] = useState<LogStreamStatus>('idle')
  const [error, setError] = useState<string | null>(null)
  const [generation, setGeneration] = useState(0)

  const seqRef = useRef(0)
  const pendingRef = useRef<T[]>([])
  const frameRef = useRef<number | null>(null)
  const maxRef = useRef(maxLines)
  useEffect(() => {
    maxRef.current = maxLines
  }, [maxLines])

  // Reset the buffer when the stream identity changes (render-time reset, no effect).
  const active = Boolean(path && enabled && signedIn)
  const streamKey = active ? `${path}|${follow}|${tail ?? ''}|${queryKey}|${generation}` : null
  const [currentKey, setCurrentKey] = useState<string | null>(null)
  if (streamKey !== currentKey) {
    setCurrentKey(streamKey)
    setLines([])
    setError(null)
    setStatus(streamKey ? 'connecting' : 'idle')
  }

  const flush = useCallback(() => {
    frameRef.current = null
    const batch = pendingRef.current
    if (batch.length === 0) return
    pendingRef.current = []
    setLines((prev) => {
      const next = prev.length + batch.length > maxRef.current ? [...prev, ...batch].slice(-maxRef.current) : [...prev, ...batch]
      return next
    })
  }, [])

  const scheduleFlush = useCallback(() => {
    if (frameRef.current !== null) return
    if (typeof document !== 'undefined' && document.visibilityState === 'hidden') {
      frameRef.current = window.setTimeout(flush, 250) as unknown as number
    } else {
      frameRef.current = window.requestAnimationFrame(flush)
    }
  }, [flush])

  const clear = useCallback(() => {
    pendingRef.current = []
    setLines([])
  }, [])

  const restart = useCallback(() => setGeneration((g) => g + 1), [])

  useEffect(() => {
    if (!path || !enabled || !signedIn) return
    const ctrl = new AbortController()
    const { signal } = ctrl
    const resumable = tail !== undefined
    const extra = queryKey ? (JSON.parse(queryKey) as Record<string, string | number | boolean>) : {}
    pendingRef.current = []

    void (async () => {
      let attempt = 0
      let firstConnect = true
      while (!signal.aborted) {
        let ended = false
        try {
          const query: Record<string, string | number | boolean> = { ...extra, follow }
          if (tail !== undefined) query.tail = firstConnect ? tail : 0
          if (!firstConnect && !resumable) {
            // finite log replayed from the start: drop what we had
            pendingRef.current = []
            setLines([])
          }
          await readSse(path, {
            query,
            signal,
            onOpen: () => {
              attempt = 0
              setError(null)
              setStatus('live')
            },
            onEvent: (ev) => {
              if (ev.event === 'end') {
                ended = true
                return
              }
              const entry = parse(ev, ++seqRef.current)
              if (entry) {
                pendingRef.current.push(entry)
                scheduleFlush()
              }
            },
          })
          if (ended || !follow) {
            flush()
            setStatus('ended')
            return
          }
          // closed without `end` (server restart): reconnect below
        } catch (e) {
          if (isAbortError(e) || signal.aborted) return
          if (e instanceof ApiError && e.status > 0 && e.status < 500) {
            flush()
            setError(e.message)
            setStatus('error')
            return
          }
          setError(errorMessage(e))
        }
        flush()
        if (!reconnect || !follow) {
          setStatus('error')
          return
        }
        firstConnect = false
        setStatus('connecting')
        try {
          await sleep(backoffDelay(attempt++), signal)
        } catch {
          return
        }
      }
    })()

    return () => {
      ctrl.abort()
      if (frameRef.current !== null) {
        window.cancelAnimationFrame(frameRef.current)
        window.clearTimeout(frameRef.current)
        frameRef.current = null
      }
    }
  }, [path, follow, tail, queryKey, parse, enabled, signedIn, reconnect, generation, flush, scheduleFlush])

  return { lines, status, error, clear, restart }
}
