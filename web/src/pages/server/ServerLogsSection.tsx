import * as React from 'react'

import { LogViewer } from '@/components/patterns/LogViewer'
import { PageHeader } from '@/components/patterns/Page'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { streamPaths } from '@/lib/api/endpoints'
import type { ServerLogLevel } from '@/lib/api/types'
import { useLogStream } from '@/lib/api/useLogStream'
import { parseServerLogEvent, SERVER_LOG_LEVELS, SERVER_LOG_LINES } from '@/lib/server-logs'

const LEVEL_LABELS: Record<ServerLogLevel, string> = { error: 'Error', warn: 'Warn', info: 'Info', debug: 'Debug' }

const EMPTY: Record<ServerLogLevel, string> = {
  error: 'No errors among the last 2,000 lines of the server.',
  warn: 'No warnings or errors among the last 2,000 lines of the server.',
  info: 'No lines yet.',
  debug: 'No lines yet.',
}

/** The level filter: this level and the more important ones. */
export function LevelSelect({ value, onChange }: { value: ServerLogLevel; onChange: (level: ServerLogLevel) => void }) {
  const id = React.useId()
  return (
    <>
      <label htmlFor={id} className="sr-only">
        Lowest level shown
      </label>
      <Select value={value} onValueChange={(v) => onChange(v as ServerLogLevel)}>
        <SelectTrigger id={id} size="sm" className="min-w-[96px]">
          <SelectValue />
        </SelectTrigger>
        <SelectContent position="popper" align="start">
          <SelectGroup>
            <SelectLabel>This level and above</SelectLabel>
            {SERVER_LOG_LEVELS.map((level) => (
              <SelectItem key={level} value={level}>
                {LEVEL_LABELS[level]}
              </SelectItem>
            ))}
          </SelectGroup>
        </SelectContent>
      </Select>
    </>
  )
}

/**
 * Server → Server logs (`?section=logs`): what ferryd logs about its own
 * work, its last 2,000 lines (kept in memory, also without a log file), then
 * each new one live. The level filter asks the server again; the search, the
 * colors, the copy buttons and the pause on scroll are the log viewer's.
 */
export function ServerLogsSection() {
  const [level, setLevel] = React.useState<ServerLogLevel>('debug')
  const stream = useLogStream(streamPaths.serverLogs, {
    query: { level },
    parse: parseServerLogEvent,
    maxLines: SERVER_LOG_LINES,
  })
  return (
    <>
      <PageHeader
        title="Server logs"
        description={
          <>
            What ferryd logs about its own work — deploys, certificates, domains — live, from its last 2,000 lines.
            It logs <code className="font-mono text-[12.5px]">info</code> and above; start it with{' '}
            <code className="font-mono text-[12.5px]">RUST_LOG=debug</code> for more. The output of each app is on
            its own Logs page.
          </>
        }
      />
      <LogViewer
        lines={stream.lines}
        status={stream.status}
        error={stream.error}
        onClear={stream.clear}
        onRestart={stream.restart}
        showInstance={false}
        download={false}
        copyLines
        height="max(360px, calc(100dvh - 300px))"
        toolbarExtra={<LevelSelect value={level} onChange={setLevel} />}
        label="Server logs"
        emptyText={EMPTY[level]}
      />
    </>
  )
}
