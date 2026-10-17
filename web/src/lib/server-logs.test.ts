import { describe, expect, it } from 'vitest'

import type { SseEvent } from './api/sse'
import { parseServerLogEvent, serverLogText, skippedText } from './server-logs'

const event = (type: string, data: unknown): SseEvent => ({
  event: type,
  data: typeof data === 'string' ? data : JSON.stringify(data),
  id: '',
})

const WARNING = {
  ts: '2026-10-17T12:00:00.250Z',
  level: 'warn',
  target: 'ferry_tls',
  message: 'certificate request failed',
  fields: { host: 'app.example.com', error: 'rate limited' },
}

describe('parseServerLogEvent', () => {
  it('turns a line into a row with its level, its target and its fields as text', () => {
    expect(parseServerLogEvent(event('log', WARNING), 7)).toEqual({
      seq: 7,
      ts: '2026-10-17T12:00:00.250Z',
      level: 'warn',
      target: 'ferry_tls',
      text: 'certificate request failed host=app.example.com error=rate limited',
    })
  })

  it('turns "skipped" into a marker that says how many lines were missed', () => {
    const marker = parseServerLogEvent(event('skipped', { count: 1500 }), 8)
    expect(marker).toMatchObject({ seq: 8, skipped: 1500, text: '1,500 lines skipped: this page fell behind the server' })
    expect(marker?.level).toBeUndefined()
  })

  it('leaves out what it cannot read', () => {
    expect(parseServerLogEvent(event('log', '{not json'), 1)).toBeNull()
    expect(parseServerLogEvent(event('log', { ...WARNING, level: 'trace' }), 1)).toBeNull()
    expect(parseServerLogEvent(event('log', { ...WARNING, message: undefined }), 1)).toBeNull()
    expect(parseServerLogEvent(event('skipped', { count: 'many' }), 1)).toBeNull()
    expect(parseServerLogEvent(event('message', WARNING), 1)).toBeNull()
  })
})

describe('serverLogText', () => {
  it('is the message, then each field, without ANSI codes', () => {
    expect(serverLogText({ ...WARNING, level: 'info', fields: {} })).toBe('certificate request failed')
    expect(serverLogText({ ...WARNING, level: 'info', message: '\u001b[31mred\u001b[0m', fields: { a: '1' } })).toBe(
      'red a=1',
    )
  })
})

describe('skippedText', () => {
  it('counts the lines', () => {
    expect(skippedText(1)).toBe('1 line skipped: this page fell behind the server')
    expect(skippedText(12000)).toBe('12,000 lines skipped: this page fell behind the server')
  })
})
