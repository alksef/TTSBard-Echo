<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { AppWindow, Minus, Mouse } from 'lucide-vue-next'
import { useWindowsSettings } from '@/composables/useAppSettings'
import type { FloatingVisibilityPayload } from '@/types'

const floatingVisible = ref(false)
const floatingPending = ref(false)
const clickthroughEnabled = ref(false)
const clickthroughPending = ref(false)
const windowsSettings = useWindowsSettings()
const snapshotVersion = ref(0)
const unlistenFns: UnlistenFn[] = []

const floatingLabel = computed(() => floatingVisible.value ? 'Скрыть плавающее окно' : 'Показать плавающее окно')
const clickthroughLabel = computed(() => clickthroughEnabled.value ? 'Выключить пропуск кликов' : 'Включить пропуск кликов')

watch(windowsSettings, settings => {
  if (settings?.floating && !clickthroughPending.value) {
    clickthroughEnabled.value = settings.floating.clickthrough
  }
}, { immediate: true })

async function refreshVisibility() {
  const version = snapshotVersion.value
  try {
    const visible = await invoke<boolean>('get_floating_visibility')
    if (version === snapshotVersion.value) floatingVisible.value = visible
  } catch (error) {
    console.error('Failed to get floating window visibility:', error)
  }
}

async function toggleFloating() {
  if (floatingPending.value) return
  floatingPending.value = true
  try {
    const payload = await invoke<FloatingVisibilityPayload>('toggle_floating_window')
    floatingVisible.value = payload.visible
  } catch (error) {
    console.error('Failed to toggle floating window:', error)
    snapshotVersion.value += 1
    await refreshVisibility()
  } finally {
    floatingPending.value = false
  }
}

async function toggleClickthrough() {
  if (clickthroughPending.value) return
  const previous = clickthroughEnabled.value
  clickthroughEnabled.value = !previous
  clickthroughPending.value = true
  try {
    clickthroughEnabled.value = await invoke<boolean>('set_clickthrough', { enabled: clickthroughEnabled.value })
  } catch (error) {
    clickthroughEnabled.value = previous
    console.error('Failed to toggle click-through:', error)
  } finally {
    clickthroughPending.value = false
  }
}

async function minimize() {
  await getCurrentWindow().minimize()
}

async function startDrag(event: MouseEvent) {
  if (event.button === 0) await getCurrentWindow().startDragging()
}

onMounted(async () => {
  unlistenFns.push(await listen<FloatingVisibilityPayload>('floating-visibility-changed', event => {
    snapshotVersion.value += 1
    floatingVisible.value = event.payload.visible
  }))
  unlistenFns.push(await listen<boolean>('clickthrough-changed', event => {
    clickthroughEnabled.value = event.payload
  }))
  await refreshVisibility()
})

onUnmounted(() => unlistenFns.splice(0).forEach(unlisten => unlisten()))
</script>

<template>
  <header class="app-titlebar" data-tauri-drag-region @mousedown="startDrag">
    <span class="titlebar-brand" data-tauri-drag-region>TTSBard Echo</span>
    <div class="titlebar-controls">
      <button class="titlebar-button clickthrough-button" :class="{ active: clickthroughEnabled }" :disabled="clickthroughPending"
        :aria-label="clickthroughLabel" :title="clickthroughLabel" :aria-pressed="clickthroughEnabled" @mousedown.stop @click="toggleClickthrough">
        <Mouse :size="15" />
      </button>
      <button class="titlebar-button" :class="{ active: floatingVisible }" :disabled="floatingPending"
        :aria-label="floatingLabel" :title="floatingLabel" @mousedown.stop @click="toggleFloating">
        <AppWindow :size="15" />
      </button>
      <button class="titlebar-button" aria-label="Свернуть" title="Свернуть" @mousedown.stop @click="minimize">
        <Minus :size="15" />
      </button>
    </div>
  </header>
</template>

<style scoped>
.app-titlebar {
  height: 36px;
  flex: 0 0 36px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 0.45rem 0 1rem;
  user-select: none;
  color: var(--color-text-secondary);
  border-bottom: 1px solid var(--color-border);
}
.titlebar-brand { font-size: .8rem; font-weight: 700; letter-spacing: .06em; }
.titlebar-controls { display: flex; align-items: center; gap: .2rem; }
.titlebar-button { width: 30px; height: 26px; display: grid; place-items: center; border-radius: 5px; color: var(--color-text-secondary); }
.clickthrough-button { margin-right: .45rem; }
.titlebar-button:hover { background: var(--color-bg-field-hover); color: var(--color-text-primary); }
.titlebar-button.active { color: var(--color-accent); }
.titlebar-button:disabled { cursor: wait; opacity: .5; }
</style>
