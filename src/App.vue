<script setup lang="ts">
import { ref } from 'vue'
import Sidebar from './components/Sidebar.vue'
import AppTitlebar from './components/AppTitlebar.vue'
import ConnectionsPanel from './components/ConnectionsPanel.vue'
import SettingsPanel from './components/SettingsPanel.vue'
import { provideAppSettings, useThemeWatcher } from './composables/useAppSettings'
import type { Panel } from '@/types'

// Provide app settings to all child components
const appSettings = provideAppSettings()

// Watch for theme changes and apply them
useThemeWatcher(appSettings)

// App state
const currentPanel = ref<Panel>('connections')
const sidebarCollapsed = ref(false)

// Listen for sidebar collapse changes
function onSidebarCollapse(collapsed: boolean) {
  sidebarCollapsed.value = collapsed
}

function setPanel(panel: Panel) {
  currentPanel.value = panel
}
</script>

<template>
  <div class="app" :class="{ 'sidebar-collapsed': sidebarCollapsed }">
    <AppTitlebar />
    <div class="content-wrapper">
      <Sidebar :panel="currentPanel" @set-panel="setPanel" @collapse="onSidebarCollapse" />
      <main class="main-content">
        <ConnectionsPanel v-if="currentPanel === 'connections'" />
        <SettingsPanel v-else-if="currentPanel === 'settings'" />
      </main>
    </div>
  </div>
</template>

<style>
/* Global app styles */
#app {
  width: 100%;
  height: 100vh;
}
</style>

<style scoped>
.app {
  display: flex;
  flex-direction: column;
  width: 100%;
  height: 100vh;
  overflow: hidden;
}

.content-wrapper {
  display: flex;
  flex: 1;
  min-height: 0;
}

.main-content {
  flex: 1;
  min-width: 0;
  padding: 1.5rem;
  overflow-y: auto;
  position: relative;
}

/* Grid pattern overlay */
.main-content::before {
  content: '';
  position: fixed;
  top: 0;
  left: var(--echo-sidebar-expanded);
  right: 0;
  bottom: 0;
  background-image: var(--grid-pattern);
  background-size: 32px 32px;
  pointer-events: none;
  mask-image: linear-gradient(to bottom, transparent, black 8%, black 92%, transparent);
  -webkit-mask-image: linear-gradient(to bottom, transparent, black 8%, black 92%, transparent);
  z-index: 0;
  transition: left 0.28s ease;
}

/* Adjust grid when sidebar is collapsed */
.sidebar-collapsed .main-content::before {
  left: var(--echo-sidebar-collapsed);
}
</style>
