import { DevupUI } from '@devup-ui/vite-plugin'
import vinext from 'vinext'
import { defineConfig } from 'vite'

export default defineConfig({
  // Diagnostic control only: IGNORE_EXTRACTED_CSS=1 suppresses the generated
  // CSS watcher events. It disables CSS hot updates; it is not a proposed fix.
  server: process.env.IGNORE_EXTRACTED_CSS === '1'
    ? { watch: { ignored: ['**/df/devup-ui/**'] } } : {},
  plugins: [DevupUI(), vinext()],
})
