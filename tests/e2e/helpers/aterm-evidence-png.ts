import { mkdirSync, writeFileSync } from 'node:fs'
import { dirname } from 'node:path'
import type { TestInfo } from '@stablyai/playwright-test'

/** Saves a canvas `toDataURL('image/png')` capture into the test's Playwright
 *  output dir — a hardcoded /tmp path doesn't exist on Windows and scatters
 *  artifacts outside test-results. Returns the written path, or undefined when
 *  the capture isn't a PNG data URL (e.g. the canvas was missing). */
export function saveEvidencePng(
  testInfo: TestInfo,
  fileName: string,
  dataUrl: string
): string | undefined {
  if (!dataUrl.startsWith('data:image/png;base64,')) {
    return undefined
  }
  const outputPath = testInfo.outputPath(fileName)
  mkdirSync(dirname(outputPath), { recursive: true })
  writeFileSync(outputPath, Buffer.from(dataUrl.split(',')[1], 'base64'))
  return outputPath
}
