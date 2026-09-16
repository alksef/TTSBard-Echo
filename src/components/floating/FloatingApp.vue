<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow, LogicalSize } from '@tauri-apps/api/window'
import FloatingConnectionList from './FloatingConnectionList.vue'
import { useConnections } from '@/composables/useConnections'
import { applyTheme } from '@/composables/useAppSettings'
import type { FloatingAppearanceDto, Theme } from '@/types'

const { connections, loading, error } = useConnections()
const appearance = ref<FloatingAppearanceDto>({
  opacity: 95,
  bg_color: '#1E1E23',
  use_custom_color: false,
  clickthrough: false,
})
const unlistenFns: UnlistenFn[] = []
let appearanceRequest = 0
let resizeObserver: ResizeObserver | undefined
let fitInProgress = false
let fitAgain = false
const connectionCount = computed(() => connections.value.length)

function applyAppearance() {
  const themeBackground = getComputedStyle(document.documentElement).getPropertyValue('--color-bg-elevated').trim()
  document.documentElement.style.setProperty('--floating-bg', appearance.value.use_custom_color ? appearance.value.bg_color : (themeBackground || '#1e1e23'))
  document.documentElement.style.setProperty('--floating-opacity', `${appearance.value.opacity}%`)
  document.documentElement.style.setProperty('--floating-clickthrough', appearance.value.clickthrough ? 'none' : 'auto')
}
watch(appearance, applyAppearance, { deep: true, immediate: true })

async function reloadAppearance() {
  const request = ++appearanceRequest
  try {
    const loaded = await invoke<FloatingAppearanceDto>('get_floating_appearance')
    if (request === appearanceRequest) appearance.value = loaded
  } catch (error) {
    console.error('[FloatingApp] Failed to load appearance:', error)
  }
}

async function loadTheme() {
  try {
    const theme = await invoke<Theme>('get_theme')
    applyTheme(theme)
    applyAppearance()
  } catch (error) {
    console.error('[FloatingApp] Failed to load theme:', error)
  }
}
async function fitToContent() {
  if (fitInProgress) {
    fitAgain = true
    return
  }

  fitInProgress = true
  try {
    do {
      fitAgain = false
      await nextTick()
      const content = document.querySelector('.connection-list')
      if (!content) return

      const window = getCurrentWindow()
      const physicalSize = await window.innerSize()
      const scaleFactor = await window.scaleFactor()
      const width = Math.max(300, Math.round(physicalSize.width / scaleFactor))
      const currentHeight = Math.round(physicalSize.height / scaleFactor)
      const contentHeight = Math.max(64, Math.min(2000, Math.ceil(content.getBoundingClientRect().height)))

      // Horizontal resizing is user-controlled; vertical resizing is always
      // corrected back to the height required by the current content.
      if (Math.abs(currentHeight - contentHeight) > 1) {
        await window.setSize(new LogicalSize(width, contentHeight))
      }
    } while (fitAgain)
  } finally {
    fitInProgress = false
  }
}
watch(connectionCount, fitToContent, { immediate: true })
async function drag(event: MouseEvent) {
  if (event.button === 0) await getCurrentWindow().startDragging()
}
onMounted(async () => {
  const window = getCurrentWindow()
  unlistenFns.push(await listen('floating-appearance-update', () => { void reloadAppearance() }))
  unlistenFns.push(await listen('clickthrough-changed', () => { void reloadAppearance() }))
  unlistenFns.push(await listen<Theme>('theme-changed', ({ payload }) => {
    applyTheme(payload)
    applyAppearance()
  }))
  await Promise.all([reloadAppearance(), loadTheme()])
  await fitToContent()
  unlistenFns.push(await window.onResized(() => { void fitToContent() }))
  const content = document.querySelector('.connection-list')
  if (content) {
    resizeObserver = new ResizeObserver(() => { void fitToContent() })
    resizeObserver.observe(content)
  }
})
onUnmounted(() => {
  unlistenFns.splice(0).forEach(unlisten => unlisten())
  resizeObserver?.disconnect()
})
</script>
<template>
  <div class="floating-app" data-tauri-drag-region @mousedown="drag">
    <FloatingConnectionList :connections="connections" :loading="loading" :error="error" />
  </div>
</template>
<style>
:root { --floating-bg: var(--color-bg-elevated, #1e1e23); --floating-opacity: 95%; --floating-clickthrough: auto; }
html, body, #app { width: 100%; min-height: 100%; margin: 0; background: transparent !important; }
body { overflow: visible; color: var(--color-text-primary); font-family: Inter, system-ui, sans-serif; }
.floating-app { display: flex; flex-direction: column; width: 100%; min-width: 300px; min-height: 64px; background: color-mix(in srgb, var(--floating-bg) var(--floating-opacity), transparent); pointer-events: var(--floating-clickthrough); user-select: none; cursor: move; }
</style>
