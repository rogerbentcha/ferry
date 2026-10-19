import * as React from 'react'
import { useNavigate } from 'react-router'
import { AlertTriangle, Globe } from 'lucide-react'
import { toast } from 'sonner'

import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogBody,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { errorMessage } from '@/lib/api/client'
import { useCreateEnvironment, useEnvironments, useServerInfo } from '@/lib/api/queries'
import type { EnvironmentView } from '@/lib/api/types'
import { previewServiceUrl } from '@/pages/services/lib'

import { environmentNameError, environmentPath } from './lib'
import { EnvironmentDnsList } from './parts'

const code = 'font-mono text-[12.5px]'

/**
 * "New environment" modal: the name, then — when the server has a domain
 * that is not local — the wildcard DNS record its web services need.
 */
export function NewEnvironmentDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const navigate = useNavigate()
  const create = useCreateEnvironment()
  const { data: environments } = useEnvironments()
  const { data: info } = useServerInfo()
  const [name, setName] = React.useState('')
  const [touched, setTouched] = React.useState(false)
  const [error, setError] = React.useState<string | null>(null)
  const [created, setCreated] = React.useState<EnvironmentView | null>(null)
  const nameId = React.useId()

  // The list is kept up to date by the change feed: follow the environment there.
  const env = created ? (environments?.find((e) => e.id === created.id) ?? created) : null

  const taken = React.useMemo(() => new Set((environments ?? []).map((e) => e.name)), [environments])
  const trimmed = name.trim()
  const nameError = environmentNameError(trimmed, taken)
  const shownNameError = touched || trimmed !== '' ? nameError : null
  const preview = previewServiceUrl('<service>', info, nameError ? 'staging' : trimmed)

  const close = () => {
    onOpenChange(false)
    setName('')
    setTouched(false)
    setError(null)
    setCreated(null)
    create.reset()
  }

  const submit = async () => {
    setTouched(true)
    if (nameError || create.isPending) return
    setError(null)
    try {
      const view = await create.mutateAsync({ name: trimmed })
      if (view.dns.length > 0) {
        // Stay open: the records to create come first.
        setCreated(view)
        return
      }
      toast.success(`Environment ${view.name} created`, {
        description: `Its web services answer at ${view.url_pattern}`,
      })
      close()
      void navigate(environmentPath(view.name))
    } catch (e) {
      setError(errorMessage(e))
    }
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (create.isPending) return
        if (o) onOpenChange(true)
        else close()
      }}
    >
      <DialogContent size={env ? 'xl' : 'lg'}>
        {env === null ? (
          <form
            noValidate
            className="flex min-h-0 flex-col"
            onSubmit={(e) => {
              e.preventDefault()
              void submit()
            }}
          >
            <DialogHeader>
              <DialogTitle>New environment</DialogTitle>
              <DialogDescription>
                A private network of its own: its services reach its datastores and services by name, and nothing of
                another environment. Pick it when you create a service or a datastore.
              </DialogDescription>
            </DialogHeader>
            <DialogBody className="gap-5">
              <div className="flex flex-col gap-1.5">
                <label htmlFor={nameId} className="text-[13px] font-medium text-foreground">
                  Name
                </label>
                <Input
                  id={nameId}
                  mono
                  autoFocus
                  autoComplete="off"
                  spellCheck={false}
                  placeholder="staging"
                  maxLength={40}
                  value={name}
                  onChange={(e) => {
                    setName(e.target.value.toLowerCase().replace(/[\s_]+/g, '-'))
                    setError(null)
                  }}
                  aria-invalid={shownNameError ? true : undefined}
                  aria-describedby={`${nameId}-${shownNameError ? 'error' : 'hint'}`}
                />
                {shownNameError ? (
                  <p id={`${nameId}-error`} role="alert" className="text-[12.5px] text-destructive">
                    {shownNameError}
                  </p>
                ) : (
                  <p id={`${nameId}-hint`} className="text-[12.5px] text-foreground-lighter">
                    Lowercase letters, digits and “-”, starting with a letter. It can’t be changed later.
                  </p>
                )}
              </div>
              {preview && (
                <p className="flex items-start gap-2 text-[13px] text-foreground-light">
                  <Globe className="mt-0.5 size-3.5 shrink-0 text-foreground-lighter" aria-hidden="true" />
                  <span>
                    Its web services will answer at{' '}
                    <span className={`${code} text-foreground [overflow-wrap:anywhere]`}>{preview}</span>
                  </span>
                </p>
              )}
              {error && (
                <div
                  role="alert"
                  className="flex items-start gap-2 rounded-md border border-destructive-border bg-destructive-soft p-3 text-[13px] text-foreground"
                >
                  <AlertTriangle className="mt-0.5 size-4 shrink-0 text-destructive" aria-hidden="true" />
                  <span className="break-words">{error}</span>
                </div>
              )}
            </DialogBody>
            <DialogFooter>
              <Button disabled={create.isPending} onClick={close}>
                Cancel
              </Button>
              <Button
                type="submit"
                variant="primary"
                loading={create.isPending}
                disabled={touched && Boolean(nameError)}
              >
                Create environment
              </Button>
            </DialogFooter>
          </form>
        ) : (
          <>
            <DialogHeader>
              <DialogTitle>{env.name} is created: point its names at this server</DialogTitle>
              <DialogDescription>
                Its web services answer at <span className={code}>{env.url_pattern}</span>. Create the wildcard record
                {env.dns.length > 1 ? 's' : ''} where the DNS of {env.dns.map((d) => d.domain).join(', ')} is managed:
                your registrar or your DNS provider.
              </DialogDescription>
            </DialogHeader>
            <DialogBody className="gap-5">
              <EnvironmentDnsList environment={env} />
            </DialogBody>
            <DialogFooter>
              <Button onClick={close}>Done</Button>
              <Button
                variant="primary"
                onClick={() => {
                  close()
                  void navigate(environmentPath(env.name))
                }}
              >
                Open {env.name}
              </Button>
            </DialogFooter>
          </>
        )}
      </DialogContent>
    </Dialog>
  )
}
