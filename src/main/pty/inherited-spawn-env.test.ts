import { describe, expect, it } from 'vitest'
import {
  CLAUDE_CODE_CHILD_SESSION_ENV_KEYS,
  INHERITED_ONLY_SPAWN_ENV_KEYS,
  cloneInheritedSpawnEnv
} from './inherited-spawn-env'

describe('cloneInheritedSpawnEnv', () => {
  it('drops every claude child-session marker and NODE_ENV, keeping other vars', () => {
    const source: NodeJS.ProcessEnv = {
      PATH: '/usr/bin',
      CLAUDECODE: '1',
      CLAUDE_CODE_CHILD_SESSION: '1',
      CLAUDE_CODE_SESSION_ID: '08a1a595-d1ec-4142-9680-0eec5fc15e17',
      CLAUDE_CODE_EXECPATH: '/home/user/.local/bin/claude',
      CLAUDE_CODE_ENTRYPOINT: 'cli',
      NODE_ENV: 'development'
    }

    const cleaned = cloneInheritedSpawnEnv(source)

    for (const key of INHERITED_ONLY_SPAWN_ENV_KEYS) {
      expect(cleaned).not.toHaveProperty(key)
    }
    expect(cleaned.PATH).toBe('/usr/bin')
    // The source env must stay untouched — callers pass live process.env.
    expect(source.CLAUDECODE).toBe('1')
    expect(source.NODE_ENV).toBe('development')
  })

  it('pins the exact marker set the claude CLI exports to children (#9155)', () => {
    expect(CLAUDE_CODE_CHILD_SESSION_ENV_KEYS).toEqual([
      'CLAUDECODE',
      'CLAUDE_CODE_CHILD_SESSION',
      'CLAUDE_CODE_SESSION_ID',
      'CLAUDE_CODE_EXECPATH',
      'CLAUDE_CODE_ENTRYPOINT'
    ])
    expect(INHERITED_ONLY_SPAWN_ENV_KEYS).toEqual([
      ...CLAUDE_CODE_CHILD_SESSION_ENV_KEYS,
      'NODE_ENV'
    ])
  })
})
