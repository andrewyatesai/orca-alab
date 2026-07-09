// Logic moved to the Rust task-providers core (orca-dispatch); this file retains types + data only.
export type TaskProvider = 'github' | 'gitlab' | 'linear' | 'jira'

export const TASK_PROVIDERS: readonly TaskProvider[] = ['github', 'gitlab', 'linear', 'jira']

export type TaskProviderAvailability = {
  gitlabInstalled: boolean
  linearConnected: boolean
}
