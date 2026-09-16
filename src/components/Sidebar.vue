<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { APP_VERSION } from '../version'
import {
  Globe,
  Settings,
  ChevronLeft,
  ChevronRight,
  LogOut
} from 'lucide-vue-next'

type Panel = 'connections' | 'settings'

import type { IconComponent } from '@/types'

interface SidebarButton {
  id: Panel
  label: string
  icon: IconComponent
}

interface SidebarGroup {
  title?: string
  buttons: SidebarButton[]
}

const props = defineProps<{
  panel: Panel
}>()

const emit = defineEmits<{
  'setPanel': [panel: Panel]
  'collapse': [collapsed: boolean]
}>()

function setPanel(panel: Panel) {
  emit('setPanel', panel)
}

// Collapse state with localStorage persistence
const STORAGE_KEY = 'sidebar-collapsed'
const isCollapsed = ref(false)
const quitPending = ref(false)

onMounted(() => {
  const saved = localStorage.getItem(STORAGE_KEY)
  if (saved !== null) {
    isCollapsed.value = saved === 'true'
    emit('collapse', isCollapsed.value)
  }
})

watch(isCollapsed, (newValue) => {
  localStorage.setItem(STORAGE_KEY, String(newValue))
  emit('collapse', newValue)
})

// Sidebar groups structure
const sidebarGroups: SidebarGroup[] = [
  {
    title: 'ГЛАВНОЕ',
    buttons: [
      { id: 'connections', label: 'Подключения', icon: Globe }
    ]
  },
  {
    buttons: [
      { id: 'settings', label: 'Настройки', icon: Settings }
    ]
  }
]

function toggleCollapse() {
  isCollapsed.value = !isCollapsed.value
}

async function quitApp() {
  if (quitPending.value) return
  quitPending.value = true
  try {
    await invoke('quit_app')
  } catch (error) {
    quitPending.value = false
    console.error('Failed to quit application:', error)
  }
}
</script>

<template>
  <aside
    class="sidebar"
    :class="{ 'sidebar-collapsed': isCollapsed }"
  >
    <!-- Floating collapse button positioned outside sidebar -->
    <button
      class="collapse-toggle-floating"
      @click="toggleCollapse"
      :title="isCollapsed ? 'Развернуть' : 'Свернуть'"
    >
      <ChevronLeft v-if="!isCollapsed" :size="18" />
      <ChevronRight v-else :size="18" />
    </button>

    <nav class="sidebar-nav">
      <template v-for="(group, groupIndex) in sidebarGroups" :key="groupIndex">
        <div
          v-for="button in group.buttons"
          :key="button.id"
          class="sidebar-button-wrapper"
        >
          <button
            class="sidebar-button"
            :class="{ 'sidebar-button-active': props.panel === button.id }"
            @click="setPanel(button.id)"
            :title="isCollapsed ? button.label : undefined"
          >
            <component :is="button.icon" :size="20" class="sidebar-icon" />
            <span v-if="!isCollapsed" class="sidebar-button-label">{{ button.label }}</span>
            <div v-if="props.panel === button.id" class="active-indicator" />
          </button>
        </div>

        <div v-if="groupIndex < sidebarGroups.length - 1" class="sidebar-divider" />
      </template>
    </nav>

    <div class="sidebar-footer">
      <div v-if="!isCollapsed" class="version-info">{{ APP_VERSION }}</div>
      <button
        class="sidebar-button quit-button"
        :disabled="quitPending"
        :title="isCollapsed ? 'Выход' : undefined"
        aria-label="Выход"
        @click="quitApp"
      >
        <LogOut :size="20" class="sidebar-icon" />
        <span v-if="!isCollapsed" class="sidebar-button-label">Выход</span>
      </button>
    </div>
  </aside>
</template>

<style scoped>
.sidebar {
  flex: 0 0 180px;
  width: 180px;
  min-width: 180px;
  position: relative;
  /* Clip the collapse control at the sidebar edge like app-tts-v2. */
  overflow: hidden;
  background:
    linear-gradient(180deg, var(--color-bg-overlay-soft), transparent 22%),
    linear-gradient(180deg, rgba(var(--rgb-sidebar-bg), 0.98) 0%, rgba(var(--rgb-sidebar-bg-bottom), 0.96) 100%);
  color: var(--color-text-primary);
  display: flex;
  flex-direction: column;
  transition: width 0.28s ease, min-width 0.28s ease;
  box-shadow: inset -1px 0 0 var(--color-border);
}

.sidebar::before {
  content: '';
  position: absolute;
  inset: 0;
  pointer-events: none;
  background:
    radial-gradient(circle at top left, rgba(var(--rgb-accent), 0.18), transparent 30%),
    linear-gradient(var(--color-grid) 1px, transparent 1px),
    linear-gradient(90deg, var(--color-grid) 1px, transparent 1px);
  background-size: auto, 18px 18px, 18px 18px;
  mask-image: linear-gradient(to bottom, rgba(0, 0, 0, 0.95) 0%, rgba(0, 0, 0, 0.7) 78%, rgba(0, 0, 0, 0.92) 100%);
}

.sidebar-collapsed {
  flex-basis: 64px;
  width: 64px;
  min-width: 64px;
}

/* Floating collapse button positioned on right edge of sidebar */
.collapse-toggle-floating {
  position: absolute;
  right: -17px;
  top: calc(70% + 36px);
  transform: translateY(-50%);
  width: 34px;
  height: 34px;
  border: 1px solid var(--color-border-strong);
  background:
    linear-gradient(135deg, var(--color-bg-elevated), var(--color-bg));
  color: var(--color-text-secondary);
  cursor: pointer;
  padding: 0;
  border-radius: 999px;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: all 0.25s ease;
  z-index: 1000;
  box-shadow:
    0 4px 16px rgba(0, 0, 0, 0.4),
    0 0 0 1px var(--color-border),
    inset 0 1px 0 var(--color-bg-overlay-soft);
}

.collapse-toggle-floating:hover {
  color: var(--color-text-primary);
  background:
    linear-gradient(135deg, var(--color-bg-elevated), var(--color-bg));
  border-color: var(--color-border-accent);
  box-shadow:
    0 6px 24px rgba(0, 0, 0, 0.5),
    0 0 0 1px rgba(var(--rgb-accent), 0.3),
    0 0 20px rgba(var(--rgb-accent), 0.25),
    inset 0 1px 0 var(--color-bg-overlay-soft);
  transform: translateY(-50%) scale(1.06);
}

.sidebar-collapsed .collapse-toggle-floating {
  right: -17px;
}

.collapse-toggle-floating svg {
  transform: translateX(-5px);
}

.sidebar-nav {
  position: relative;
  z-index: 1;
  display: flex;
  flex-direction: column;
  flex: 1;
  overflow-y: auto;
  padding: 1rem 0 0.75rem;
}

.sidebar-button-wrapper {
  position: relative;
  margin-bottom: 0;
}

.sidebar-button {
  width: 100%;
  min-height: 30px;
  padding: 0 0.85rem 0 1rem;
  border: 1px solid transparent;
  background: var(--color-bg-overlay);
  color: var(--color-text-secondary);
  cursor: pointer;
  text-align: left;
  transition: all 0.18s ease;
  display: flex;
  align-items: center;
  gap: 0.75rem;
  position: relative;
  border-radius: 0;
  backdrop-filter: blur(8px);
}

.sidebar-button:hover {
  background: var(--color-bg-field-hover);
  color: var(--color-text-primary);
  border-color: var(--color-border);
}

.sidebar-button-active {
  background: rgba(var(--rgb-border), 0.09);
  border-color: var(--color-border);
  color: var(--color-text-primary) !important;
  box-shadow: inset 0 1px 0 var(--color-border);
}

.sidebar-button-active .sidebar-icon {
  color: var(--color-text-primary);
}

.active-indicator {
  position: absolute;
  left: 0;
  top: 50%;
  transform: translateY(-50%);
  width: 6px;
  height: 1.75rem;
  background: linear-gradient(180deg, var(--color-accent) 0%, var(--color-accent-strong) 100%);
  border-radius: 0 999px 999px 0;
  box-shadow: var(--shadow-accent-glow);
}

.sidebar-divider {
  height: 1px;
  background: var(--color-border);
  margin: 1rem 0 0.85rem;
}

.sidebar-collapsed .sidebar-divider {
  margin: 0.75rem 0 0.6rem;
}

.sidebar-icon {
  min-width: 20px;
  flex-shrink: 0;
}

.sidebar-collapsed .sidebar-icon {
  margin: 0;
}

.sidebar-button-label {
  flex: 1;
  font-size: 0.92rem;
  font-weight: 600;
  line-height: 1;
  display: flex;
  align-items: center;
}

.sidebar-footer {
  position: relative;
  z-index: 1;
  padding: 0.7rem 0 0.85rem;
  border-top: 1px solid var(--color-border);
  display: flex;
  flex-direction: column;
  gap: 0.45rem;
  margin-top: auto;
}

.version-info {
  font-size: 0.76rem;
  color: var(--color-text-muted);
  font-family: var(--font-mono);
  padding: 0 1rem;
}

.quit-button {
  justify-content: center;
  color: var(--color-danger);
  background: rgba(var(--rgb-danger), 0.05);
  border-color: rgba(var(--rgb-danger), 0.12);
}

.quit-button:hover {
  background: rgba(var(--rgb-danger), 0.12);
  color: var(--color-danger);
}

.quit-button:disabled {
  cursor: wait;
  opacity: 0.5;
}

.sidebar-collapsed .version-info {
  display: none;
}

.sidebar-collapsed .quit-button {
  justify-content: center;
  padding: 0.8rem;
}

.sidebar-collapsed .sidebar-nav {
  padding-left: 0;
  padding-right: 0;
}

.sidebar-collapsed .sidebar-button {
  justify-content: center;
  padding: 0;
}

.sidebar-collapsed .active-indicator {
  left: 0;
}

/* Only stack navigation on genuinely narrow mobile widths. The compact
   desktop window is compact and must still show sidebar + content. */
@media (max-width: 420px) {
  .sidebar,
  .sidebar-collapsed {
    width: 100%;
    min-width: 100%;
    flex-basis: auto;
  }

  .sidebar {
    box-shadow: inset 0 -1px 0 var(--color-border);
  }

  .sidebar-nav {
    padding-bottom: 1.2rem;
  }

  .collapse-toggle-floating {
    display: none;
  }
}
</style>
