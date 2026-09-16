/* ============================================================================
   Tauri API test double.

   Test files mock the @tauri-apps/api modules with this module:
     vi.mock('@tauri-apps/api/core', async () => await import('@/test/helpers/tauri'))
     vi.mock('@tauri-apps/api/event', async () => await import('@/test/helpers/tauri'))
     vi.mock('@tauri-apps/api/window', async () => await import('@/test/helpers/tauri'))
   and then control/assert through the exported fns below. Nothing here touches
   a real WebView, IPC channel or network.
   ============================================================================ */
import { vi } from 'vitest'

/* ==========================================================================
   invoke: routes by command name. Unhandled commands fail loudly so a test
   can never pass by silently hitting the wrong backend call.
   ========================================================================== */
type InvokeArgs = Record<string, unknown> | undefined
type CommandHandler = (args: InvokeArgs) => unknown

const commandHandlers = new Map<string, CommandHandler>()

export const invoke = vi.fn((command: string, args?: InvokeArgs): Promise<unknown> => {
  const handler = commandHandlers.get(command)
  if (!handler) return Promise.reject(new Error(`Unexpected invoke('${command}') — register it with onInvoke()`))
  // Real invoke always returns a promise; handler throws become rejections.
  return Promise.resolve().then(() => handler(args))
})

export function onInvoke(command: string, handler: CommandHandler): void {
  commandHandlers.set(command, handler)
}

/** Response for a command; value or a (sync/async) error thrower. */
export function invokeReturns(command: string, value: unknown): void {
  onInvoke(command, () => value)
}

export function invokeRejects(command: string, reason: unknown): void {
  onInvoke(command, () => { throw reason })
}

/** Replace a handler with one that never settles until the test resolves it. */
export function invokePending(command: string): { resolve: (value: unknown) => void; reject: (reason: unknown) => void } {
  let resolve!: (value: unknown) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<unknown>((res, rej) => { resolve = res; reject = rej })
  onInvoke(command, () => promise)
  return { resolve, reject }
}

export function invokeCalls(command: string): number {
  return invoke.mock.calls.filter(([name]) => name === command).length
}

/* ==========================================================================
   listen: registers handlers in an in-memory bus; tests emit through
   emitTauriEvent() and assert registration through tauriListenerCount().
   ========================================================================== */
type TauriEventListener = (event: { payload: unknown }) => void

const listeners = new Map<string, Set<TauriEventListener>>()

export const listen = vi.fn((event: string, handler: TauriEventListener): Promise<() => void> => {
  let set = listeners.get(event)
  if (!set) { set = new Set(); listeners.set(event, set) }
  set.add(handler)
  return Promise.resolve(() => { set.delete(handler) })
})

export function emitTauriEvent(event: string, payload?: unknown): void {
  for (const handler of [...(listeners.get(event) ?? [])]) handler({ payload })
}

export function tauriListenerCount(event?: string): number {
  if (event) return listeners.get(event)?.size ?? 0
  let total = 0
  for (const set of listeners.values()) total += set.size
  return total
}

/* ==========================================================================
   Window API: every getCurrentWindow() handle shares this object.
   ========================================================================== */
export const windowApi = {
  minimize: vi.fn<() => Promise<void>>(async () => {}),
  close: vi.fn<() => Promise<void>>(async () => {}),
  startDragging: vi.fn<() => Promise<void>>(async () => {}),
  innerSize: vi.fn<() => Promise<{ width: number; height: number }>>(async () => ({ width: 700, height: 200 })),
  scaleFactor: vi.fn<() => Promise<number>>(async () => 1),
  setSize: vi.fn<(size: unknown) => Promise<void>>(async () => {}),
  onResized: vi.fn<(handler: (event: { payload: { width: number; height: number } }) => void) => Promise<() => void>>(async () => () => {}),
}

export const getCurrentWindow = vi.fn(() => windowApi)

export class LogicalSize {
  constructor(public width: number, public height: number) {}
}

/* ==========================================================================
   ResizeObserver stub with per-instance fns so tests can assert disconnect.
   ========================================================================== */
export interface ResizeObserverStub {
  observe: ReturnType<typeof vi.fn>
  unobserve: ReturnType<typeof vi.fn>
  disconnect: ReturnType<typeof vi.fn>
}

const resizeObservers: ResizeObserverStub[] = []

// Exported as a class so setup can install it as the global ResizeObserver.
export class ResizeObserverMock implements ResizeObserverStub {
  observe = vi.fn()
  unobserve = vi.fn()
  disconnect = vi.fn()
  constructor(_callback: ResizeObserverCallback) {
    resizeObservers.push(this)
  }
}

export function createdResizeObservers(): ResizeObserverStub[] {
  return [...resizeObservers]
}

/* ==========================================================================
   Reset between tests. invoke's routing implementation is kept; only the
   per-test handlers, event bus and call history are cleared.
   ========================================================================== */
export function resetTauriMocks(): void {
  commandHandlers.clear()
  invoke.mockClear()
  listen.mockClear()
  listeners.clear()
  for (const fn of Object.values(windowApi)) fn.mockClear()
  getCurrentWindow.mockClear()
  resizeObservers.length = 0
}
