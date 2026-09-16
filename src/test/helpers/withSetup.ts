/* ============================================================================
   Run a composable inside a real component setup so lifecycle hooks
   (onUnmounted/onScopeDispose) work, and hand back the instance for
   deterministic teardown.
   ============================================================================ */
import { createApp, h, type App } from 'vue'

export interface SetupHandle<T> {
  result: T
  app: App
  unmount: () => void
}

export function withSetup<T>(composable: () => T): SetupHandle<T> {
  let result!: T
  const app = createApp({
    setup() {
      result = composable()
      // Suppress "Component is missing template" warning.
      return () => h('div')
    },
  })
  app.mount(document.createElement('div'))
  return {
    result,
    app,
    unmount: () => app.unmount(),
  }
}
