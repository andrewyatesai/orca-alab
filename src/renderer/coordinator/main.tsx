import '../src/assets/main.css'

import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { App } from './App'

// No settings store in this window: follow the OS scheme so the design tokens
// resolve (main.css keys its dark values off the root `dark` class).
const scheme = window.matchMedia('(prefers-color-scheme: dark)')
const applyScheme = (): void => {
  document.documentElement.classList.toggle('dark', scheme.matches)
}
applyScheme()
scheme.addEventListener('change', applyScheme)

createRoot(document.getElementById('root') as HTMLElement).render(
  <StrictMode>
    <App />
  </StrictMode>
)
