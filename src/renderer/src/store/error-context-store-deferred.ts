// Error boundaries can run during store initialization, so crash reporting
// reads the store through a deferred facade instead of creating an eager cycle.
export { useAppStore } from './index'
