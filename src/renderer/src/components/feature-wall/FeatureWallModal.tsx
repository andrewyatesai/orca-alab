import type { JSX } from 'react'
import { getFeatureWallOpenSource } from './feature-wall-modal-helpers'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle
} from '@/components/ui/dialog'
import { useAppStore } from '@/store'
import { FeatureWallTourSurface } from './FeatureWallTourSurface'
import { translate } from '@/i18n/i18n'

export default function FeatureWallModal(): JSX.Element | null {
  const activeModal = useAppStore((s) => s.activeModal)
  const modalData = useAppStore((s) => s.modalData)
  const closeModal = useAppStore((s) => s.closeModal)
  const openModal = useAppStore((s) => s.openModal)
  const isOpen = activeModal === 'feature-wall'
  const source = getFeatureWallOpenSource(modalData)

  const handleOpenChange = (open: boolean): void => {
    if (!open) {
      closeModal()
    }
  }

  if (!isOpen) {
    return null
  }

  return (
    <Dialog open={isOpen} onOpenChange={handleOpenChange}>
      <DialogContent
        className="grid h-[min(780px,calc(100vh-2rem))] w-[min(1240px,calc(100vw-2rem))] max-w-none grid-rows-[auto_minmax(0,1fr)] gap-0 p-0 motion-reduce:data-[state=closed]:animate-none motion-reduce:data-[state=open]:animate-none sm:max-w-none"
        overlayClassName="motion-reduce:data-[state=closed]:animate-none motion-reduce:data-[state=open]:animate-none"
        tabIndex={-1}
      >
        {/* Why: at Orca's supported minimum height, preserving a visual preview
            is more useful than spending the first fold on modal chrome. */}
        <DialogHeader className="gap-0.5 border-b border-border px-4 py-3 [@media(max-height:500px)]:py-2 md:gap-1 md:px-7 md:py-3">
          <DialogTitle className="text-base md:text-lg">
            {translate(
              'auto.components.feature.wall.FeatureWallModal.3567e147c8',
              'Explore Orca: ALab Edition'
            )}
          </DialogTitle>
          <DialogDescription className="text-[11px] text-muted-foreground md:text-xs">
            {translate(
              'auto.components.feature.wall.FeatureWallModal.a140000001',
              '14 guided screens · about 7 minutes'
            )}
          </DialogDescription>
        </DialogHeader>

        <FeatureWallTourSurface
          isOpen={isOpen}
          source={source}
          doneLabel={translate(
            'auto.components.feature.wall.FeatureWallModal.a120000001',
            'Return to Orca'
          )}
          finalSecondaryLabel={translate(
            'auto.components.feature.wall.FeatureWallModal.a130000001',
            'Finish setup'
          )}
          onFinalSecondaryAction={() =>
            openModal('setup-guide', { telemetrySource: 'feature_wall' })
          }
          onDone={closeModal}
        />
      </DialogContent>
    </Dialog>
  )
}
