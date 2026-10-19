import { toast } from 'sonner'

import { ConfirmDialog } from '@/components/patterns/ConfirmDialog'
import { Callout } from '@/components/patterns/EmptyState'
import { useDeleteEnvironment } from '@/lib/api/queries'
import type { EnvironmentView } from '@/lib/api/types'

interface DeleteEnvironmentDialogProps {
  environment: Pick<EnvironmentView, 'name' | 'services' | 'datastores'>
  open: boolean
  onOpenChange: (open: boolean) => void
  onDeleted?: () => void
}

/**
 * Typed-name confirmation. The server refuses (409) while the environment
 * holds services or datastores; the dialog says so beforehand when it knows,
 * and shows the server's message otherwise. Production has no delete action.
 */
export function DeleteEnvironmentDialog({ environment, open, onOpenChange, onDeleted }: DeleteEnvironmentDialogProps) {
  const del = useDeleteEnvironment()
  const held = [
    ...environment.services.map((s) => `service ${s}`),
    ...environment.datastores.map((d) => `datastore ${d}`),
  ]

  const confirm = async () => {
    // A 409 rejects with the server's message, shown by the dialog.
    await del.mutateAsync(environment.name)
    toast.success(`Environment ${environment.name} deleted`)
    onDeleted?.()
  }

  return (
    <ConfirmDialog
      open={open}
      onOpenChange={onOpenChange}
      title={`Delete environment ${environment.name}`}
      description={
        <p>
          This removes the environment and its private network. Services and datastores are never deleted with it: an
          environment can only be deleted once it is empty.
        </p>
      }
      confirmText={environment.name}
      confirmLabel="Delete environment"
      onConfirm={confirm}
    >
      {held.length > 0 && (
        <Callout tone="warning" title={`${environment.name} is not empty`}>
          <p>
            It still holds <span className="font-medium text-foreground">{held.join(', ')}</span>. Delete them first.
          </p>
        </Callout>
      )}
    </ConfirmDialog>
  )
}
