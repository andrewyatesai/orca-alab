import React, { Suspense } from 'react'
import { cn } from '@/lib/utils'
import { lazyWithRetry } from '@/lib/lazy-with-retry'
import type { CommentMarkdownProps } from './comment-markdown-content'

export type { CommentMarkdownLinkClickHandler } from './comment-markdown-element-renderers'
export type { CommentMarkdownProps } from './comment-markdown-content'

// Why lazy: the markdown pipeline behind this component (react-markdown +
// rehype-raw/parse5 + sanitize, ~460KB) was the largest deferrable block in the
// renderer's eager first-paint chunk — sidebar/dashboard render CommentMarkdown
// at boot, so a static import dragged the whole pipeline into every launch's
// parse. The fallback shows the raw text under the same className so layout
// (single-line preview truncation, reserved heights) holds through the one
// cold-start chunk load.
const CommentMarkdownContent = lazyWithRetry(() => import('./comment-markdown-content'))

const CommentMarkdown = React.memo(
  React.forwardRef<HTMLDivElement, CommentMarkdownProps>(function CommentMarkdown(props, ref) {
    const {
      content,
      className,
      variant: _variant,
      githubRepo: _githubRepo,
      onLinkClick: _onLinkClick,
      allowFileUriLinks: _allowFileUriLinks,
      ...rest
    } = props
    return (
      <Suspense
        fallback={
          // Same ref + DOM-prop spread as the real component: Radix asChild
          // (HoverCardTrigger) merges a ref and handlers onto the child, and
          // must keep working during the load window.
          <div
            ref={ref}
            className={cn('min-w-0 max-w-full [overflow-wrap:anywhere]', className)}
            {...rest}
          >
            {content}
          </div>
        }
      >
        <CommentMarkdownContent {...props} ref={ref} />
      </Suspense>
    )
  })
)

export default CommentMarkdown
