<script setup lang="ts">
import { ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { AlertTriangle } from 'lucide-vue-next'
import { useGeneralSettings, useLoggingSettings } from '@/composables/useAppSettings'
import StatusMessage from '@/components/shared/StatusMessage.vue'

const generalSettings = useGeneralSettings()
const loggingSettings = useLoggingSettings()
const statusMessage = ref('')
const statusType = ref<'success' | 'error' | 'info' | 'warning'>('info')
const logLevelOptions = ['trace', 'debug', 'info', 'warn', 'error']
const selectedLogLevel = ref('info')
const messageClearInterval = ref(30)
const hideOnMinimize = ref(false)
const hideOnMinimizeSaving = ref(false)
watch(loggingSettings, settings => { if (settings) selectedLogLevel.value = settings.level }, { immediate: true })
watch(generalSettings, settings => { if (settings) { messageClearInterval.value = settings.message_clear_interval_seconds; hideOnMinimize.value = settings.hide_on_minimize } }, { immediate: true })
function showStatus(message: string, type: 'success' | 'error' | 'info' | 'warning') { statusMessage.value = message; statusType.value = type === 'success' ? 'warning' : type }
async function update(command: string, args: Record<string, unknown>, success: string) { try { await invoke(command, args); showStatus(success, 'success') } catch (error) { showStatus(`Не удалось сохранить настройку: ${String(error)}`, 'error') } }
async function setMessageClearInterval() { const previous = generalSettings.value?.message_clear_interval_seconds ?? 30; const seconds = Math.round(messageClearInterval.value); if (!Number.isFinite(seconds) || seconds < 1 || seconds > 3600) { messageClearInterval.value = previous; showStatus('Интервал должен быть от 1 до 3600 секунд', 'error'); return }; busy.value = true; try { await invoke('set_message_clear_interval', { seconds }); showStatus('Интервал очистки сообщений обновлён', 'success') } catch (error) { messageClearInterval.value = previous; showStatus(`Не удалось сохранить интервал: ${String(error)}`, 'error') } finally { busy.value = false } }
const busy = ref(false)
async function toggleHideOnMinimize() { if (hideOnMinimizeSaving.value) return; const previous = hideOnMinimize.value; hideOnMinimize.value = !previous; hideOnMinimizeSaving.value = true; try { await invoke('set_hide_on_minimize', { value: hideOnMinimize.value }); showStatus('Поведение сворачивания обновлено', 'success') } catch (error) { hideOnMinimize.value = previous; showStatus(`Не удалось сохранить настройку: ${String(error)}`, 'error') } finally { hideOnMinimizeSaving.value = false } }
</script>

<template>
  <div class="settings-general">
    <StatusMessage :message="statusMessage" :type="statusType" @dismiss="statusMessage = ''" />
    <section class="settings-section">
      <div class="setting-row"><label class="setting-label checkbox-label"><input :checked="hideOnMinimize" :disabled="hideOnMinimizeSaving" type="checkbox" class="checkbox-input" @change="toggleHideOnMinimize" /><span>Скрывать с панели задач при сворачивании</span></label><span class="setting-hint">Если выключено, свёрнутое окно остаётся на панели задач. Приложение продолжает работать в обоих режимах.</span></div>
    </section>
    <section class="settings-section">
      <div class="setting-row"><label class="setting-label checkbox-label"><input :checked="loggingSettings?.enabled" type="checkbox" class="checkbox-input" @change="update('set_logging_enabled', { enabled: ($event.target as HTMLInputElement).checked }, 'Логирование обновлено')" /><span>Включить логирование</span></label></div>
      <div v-if="loggingSettings?.enabled" class="setting-group"><div class="setting-row"><label class="inline-label" for="log-level">Уровень:</label><select id="log-level" :value="selectedLogLevel" class="level-select" @change="selectedLogLevel = ($event.target as HTMLSelectElement).value; update('set_logging_level', { level: selectedLogLevel }, 'Уровень логирования сохранён')"><option v-for="level in logLevelOptions" :key="level" :value="level">{{ level.toUpperCase() }}</option></select></div></div>
      <span class="setting-warning"><AlertTriangle :size="14" /> Требуется перезапуск приложения для применения изменений</span>
    </section>
    <section class="settings-section">
      <div class="setting-row"><label class="setting-label checkbox-label"><input :checked="generalSettings?.exclude_from_capture" type="checkbox" class="checkbox-input" @change="update('set_exclude_from_capture', { exclude: ($event.target as HTMLInputElement).checked }, 'Настройка захвата обновлена')" /><span>Скрыть от захвата экрана</span></label><span class="setting-hint">Исключает окно из системных средств записи экрана.</span><span class="setting-warning"><AlertTriangle :size="14" /> Требуется перезапуск</span></div>
    </section>
    <section class="settings-section">
      <div class="setting-row"><label class="inline-label" for="message-clear-interval">Очищать сообщения через</label><div class="interval-control"><input id="message-clear-interval" v-model.number="messageClearInterval" type="number" min="1" max="3600" step="1" class="text-input interval-input" :disabled="busy" @change="setMessageClearInterval" @keyup.enter="setMessageClearInterval" /><span>сек.</span></div><span class="setting-hint no-indent">Последнее сообщение исчезнет из основного и плавающего окон.</span></div>
    </section>
  </div>
</template>

<style scoped>
.settings-general { display: flex; flex-direction: column; gap: 1.5rem; }.settings-section { padding: 12px 16px; background: var(--color-bg-field); border: 1px solid var(--color-border); border-radius: 12px; backdrop-filter: blur(8px); }.setting-row { display: block; margin-bottom: 1rem; }.setting-row:last-child { margin-bottom: 0; }.setting-label { display: flex; align-items: center; gap: .6rem; cursor: pointer; user-select: none; font-size: .95rem; font-weight: 600; color: var(--color-text-primary); }.checkbox-input { width: 18px; height: 18px; cursor: pointer; accent-color: var(--color-accent); }.setting-hint { display: block; margin: .4rem 0 0 2.4rem; font-size: .85rem; color: var(--color-text-muted); line-height: 1.4; }.setting-group { margin-top: 1rem; padding-left: 2.4rem; }.inline-label { display: inline-block; margin-right: .6rem; font-size: .9rem; color: var(--color-text-primary); }.level-select { min-width: 140px; padding: .4rem .6rem; border: 1px solid var(--color-border-strong); border-radius: 6px; background: var(--color-bg-field-hover); color: var(--color-text-primary); font-size: .9rem; cursor: pointer; color-scheme: dark; }.level-select option { background: var(--color-bg-elevated); color: var(--color-text-primary); }.level-select:focus { outline: none; border-color: var(--color-accent); box-shadow: 0 0 0 2px var(--focus-glow); }
.setting-warning { display: flex; align-items: center; gap: .4rem; margin: .5rem 0 0 2.4rem; color: var(--warning-text-bright); font-size: .82rem; }
.interval-control { display: flex; align-items: center; gap: .75rem; color: var(--color-text-secondary); }
.text-input { padding: .6rem; border: 1px solid var(--color-border-strong); border-radius: 10px; background: var(--color-bg-field); color: var(--color-text-primary); font: inherit; }
.text-input:focus { outline: none; border-color: var(--color-accent); box-shadow: 0 0 0 2px var(--focus-glow); }
.interval-input { width: 96px; }
:global([data-theme='light']) .level-select { color-scheme: light; }
</style>
