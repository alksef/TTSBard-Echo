<script setup lang="ts">
import { ref } from 'vue'
import { Settings, Palette } from 'lucide-vue-next'
import SettingsGeneral from './settings/SettingsGeneral.vue'
import SettingsInterface from './settings/SettingsInterface.vue'

type TabType = 'general' | 'interface'
const activeTab = ref<TabType>('general')
</script>

<template>
  <div class="settings-panel">
    <div class="settings-tabs" role="tablist" aria-label="Настройки">
      <button class="settings-tab" :class="{ active: activeTab === 'general' }" type="button" role="tab" :aria-selected="activeTab === 'general'" @click="activeTab = 'general'">
        <Settings :size="18" /><span>Общие</span>
      </button>
      <button class="settings-tab" :class="{ active: activeTab === 'interface' }" type="button" role="tab" :aria-selected="activeTab === 'interface'" @click="activeTab = 'interface'">
        <Palette :size="18" /><span>Интерфейс</span>
      </button>
    </div>
    <Transition name="fade" mode="out-in">
      <SettingsGeneral v-if="activeTab === 'general'" key="general" />
      <SettingsInterface v-else key="interface" />
    </Transition>
  </div>
</template>

<style scoped>
.settings-panel { max-width: 900px; margin: 0 auto; }
.settings-tabs { display: flex; gap: .5rem; margin-bottom: 1.5rem; border-bottom: 1px solid var(--color-border); padding-bottom: .5rem; }
.settings-tab { display: flex; align-items: center; gap: .5rem; padding: .5rem 1rem; border: 0; border-radius: 8px 8px 0 0; background: transparent; color: var(--color-text-secondary); cursor: pointer; font-size: .9rem; font-weight: 500; transition: all .2s; }
.settings-tab:hover { color: var(--color-text-primary); background: var(--color-bg-field-hover); }
.settings-tab.active { color: var(--color-accent); background: var(--color-bg-field); border-bottom: 2px solid var(--color-accent); }
.fade-enter-active, .fade-leave-active { transition: opacity .2s ease; }.fade-enter-from, .fade-leave-to { opacity: 0; }
</style>
