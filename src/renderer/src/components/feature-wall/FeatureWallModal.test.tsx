import type { ReactNode } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'
import FeatureWallModal from './FeatureWallModal'

const mocks = vi.hoisted(() => ({
  closeModal: vi.fn(),
  openModal: vi.fn(),
  tourProps: null as null | {
    onDone: () => void
    onFinalSecondaryAction: () => void
  },
  state: {
    activeModal: 'feature-wall',
    modalData: { source: 'help_menu' }
  }
}))

vi.mock('@/store', () => ({
  useAppStore: (
    selector: (
      state: typeof mocks.state & {
        closeModal: () => void
        openModal: (...args: unknown[]) => void
      }
    ) => unknown
  ) => selector({ ...mocks.state, closeModal: mocks.closeModal, openModal: mocks.openModal })
}))

vi.mock('@/i18n/i18n', () => ({
  translate: (_key: string, fallback: string) => fallback
}))

vi.mock('@/components/ui/dialog', () => ({
  Dialog: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  DialogContent: ({
    children,
    className,
    overlayClassName
  }: {
    children: ReactNode
    className?: string
    overlayClassName?: string
  }) => (
    <section className={className} data-overlay-class-name={overlayClassName}>
      {children}
    </section>
  ),
  DialogDescription: ({ children }: { children: ReactNode }) => <p>{children}</p>,
  DialogHeader: ({ children }: { children: ReactNode }) => <header>{children}</header>,
  DialogTitle: ({ children }: { children: ReactNode }) => <h1>{children}</h1>
}))

vi.mock('./FeatureWallTourSurface', () => ({
  FeatureWallTourSurface: (props: { onDone: () => void; onFinalSecondaryAction: () => void }) => {
    mocks.tourProps = props
    return <div data-testid="feature-wall-tour" />
  }
}))

describe('FeatureWallModal', () => {
  it('removes entrance and exit animation when reduced motion is requested', () => {
    const html = renderToStaticMarkup(<FeatureWallModal />)

    expect(html).toContain(
      'motion-reduce:data-[state=closed]:animate-none motion-reduce:data-[state=open]:animate-none'
    )
    expect(html).toContain(
      'data-overlay-class-name="motion-reduce:data-[state=closed]:animate-none motion-reduce:data-[state=open]:animate-none"'
    )
  })

  it('wires Return to Orca and Finish setup to distinct destinations', () => {
    renderToStaticMarkup(<FeatureWallModal />)

    mocks.tourProps?.onDone()
    expect(mocks.closeModal).toHaveBeenCalledOnce()

    mocks.tourProps?.onFinalSecondaryAction()
    expect(mocks.openModal).toHaveBeenCalledWith('setup-guide', {
      telemetrySource: 'feature_wall'
    })
  })
})
