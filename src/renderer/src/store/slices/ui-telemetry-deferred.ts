// The UI slice is imported while the store is being assembled. Keep telemetry
// deferred without asking Rollup to split the already-shared module.
export { track } from '@/lib/telemetry'
