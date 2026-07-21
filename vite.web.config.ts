import { resolve } from 'node:path'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import {
  createRendererChunkBudgetPlugin,
  createRendererWorkerChunkBudgetPlugin
} from './build-plugins/renderer-chunk-budget'

export default defineConfig({
  root: resolve('src/renderer'),
  // Why: pairing URLs may live under a reverse-proxy path prefix like
  // /orca/web-index.html, so built assets must resolve relative to the page.
  base: './',
  plugins: [react(), tailwindcss(), createRendererChunkBudgetPlugin('web')],
  define: {
    ORCA_FEATURE_WALL_ENABLED: 'true'
  },
  resolve: {
    alias: {
      '@renderer': resolve('src/renderer/src'),
      '@': resolve('src/renderer/src')
    }
  },
  build: {
    outDir: resolve('out/web'),
    emptyOutDir: true,
    // The custom policy caps both the 2 MiB entry closure and lazy files rather
    // than relying on Vite's one-size-fits-all 500 kB advisory.
    chunkSizeWarningLimit: 5_000,
    rollupOptions: {
      input: resolve('src/renderer/web-index.html')
    }
  },
  worker: {
    format: 'es',
    plugins: () => [createRendererWorkerChunkBudgetPlugin()]
  }
})
