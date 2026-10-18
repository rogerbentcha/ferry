import { FerryLogo } from '@/components/ferry-logo'

/** The Ferry logo: the same image as the dashboard's and the docs', a little transparent in dark mode. */
export function FerryMark({ className }: { className?: string }) {
  return <FerryLogo className={className ?? ''} />
}

export function Wordmark() {
  return (
    <span className="flex items-center gap-2">
      <FerryMark className="size-7" />
      <span className="font-display text-[1.3125rem] font-bold tracking-[-0.02em]">Ferry</span>
    </span>
  )
}
