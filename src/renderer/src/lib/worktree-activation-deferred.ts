// The repos slice is part of store construction and worktree-activation imports
// the store root. This facade keeps that cycle boundary explicitly deferred.
export { activateAndRevealWorktree } from './worktree-activation'
