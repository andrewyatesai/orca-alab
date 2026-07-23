import { spawnSync } from 'node:child_process'

const boundary = '([^A-Za-z0-9_-]|$)'
const retiredRepositories = [
  ['andrewyatesai', 'orc'].join('/'),
  ['andrewyatesai', 'aterm'].join('/')
]

let foundRetiredReference = false
for (const repository of retiredRepositories) {
  const result = spawnSync(
    'git',
    ['grep', '-n', '-I', '-E', `${repository}${boundary}`, '--', '.'],
    { encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 }
  )
  if (result.status === 1) {
    continue
  }
  if (result.status !== 0) {
    throw new Error(result.stderr || `git grep failed while checking ${repository}`)
  }
  for (const line of result.stdout.trimEnd().split('\n')) {
    foundRetiredReference = true
    console.error(`[repository-identity] ${line}: retired reference ${repository}`)
  }
}

if (foundRetiredReference) {
  process.exitCode = 1
} else {
  console.log(
    '[repository-identity] ok — development and public dependency repository names are canonical.'
  )
}
