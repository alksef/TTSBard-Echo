<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { Moon, Sun } from 'lucide-vue-next'
import StatusMessage from '@/components/shared/StatusMessage.vue'
import { applyTheme, useTheme } from '@/composables/useAppSettings'
import { opacityToTransparency, transparencyToOpacity } from '@/types/settings'
import type { FloatingAppearanceDto } from '@/types'

const currentTheme = useTheme(); const selectedTheme = ref(currentTheme.value); const transparency = ref(10); const bgColor = ref('#000000'); const colorText = ref('#000000'); const useCustomColor = ref(false); const busy = ref(false); const statusMessage = ref(''); const statusType = ref<'success' | 'error' | 'info'>('info'); let appearanceRequest = 0
watch(currentTheme, value => { selectedTheme.value = value }, { immediate: true })
function applyAppearance(value: FloatingAppearanceDto) { transparency.value = opacityToTransparency(value.opacity); bgColor.value = value.bg_color; colorText.value = value.bg_color; useCustomColor.value = value.use_custom_color }
async function loadAppearance() { const request = appearanceRequest; try { const value = await invoke<FloatingAppearanceDto>('get_floating_appearance'); if (request === appearanceRequest) applyAppearance(value) } catch (error) { status(`Не удалось загрузить настройки окна: ${String(error)}`, 'error') } }
function status(message: string, type: 'success' | 'error' | 'info') { statusMessage.value = message; statusType.value = type }
function isHex(value: string) { return /^#[0-9a-f]{6}$/i.test(value) }
async function setTheme() { const nextTheme = selectedTheme.value; try { await invoke('update_theme', { theme: nextTheme }); applyTheme(nextTheme) } catch (error) { selectedTheme.value = currentTheme.value; status('Не удалось изменить тему: ' + String(error), 'error') } }
async function setTransparency(value: number) { if (busy.value) return; transparency.value = Math.min(90, Math.max(0, value)); const request = ++appearanceRequest; busy.value = true; try { const applied = await invoke<FloatingAppearanceDto>('set_floating_opacity', { value: transparencyToOpacity(transparency.value) }); if (request === appearanceRequest) applyAppearance(applied) } catch (error) { status(`Не удалось сохранить прозрачность: ${String(error)}`, 'error'); if (request === appearanceRequest) void loadAppearance() } finally { if (request === appearanceRequest) busy.value = false } }
async function setColor(value: string) {
  if (busy.value) return
  if (!isHex(value)) { status('Цвет должен быть в формате #RRGGBB', 'error'); return }
  const nextColor = value.toUpperCase()
  const previousColor = bgColor.value
  const previousCustom = useCustomColor.value
  bgColor.value = nextColor
  colorText.value = nextColor
  const request = ++appearanceRequest
  busy.value = true
  try {
    const applied = await invoke<FloatingAppearanceDto>('set_floating_bg_color', { color: nextColor })
    if (request === appearanceRequest) applyAppearance(applied)
  } catch (error) {
    if (request === appearanceRequest) { bgColor.value = previousColor; colorText.value = previousColor; useCustomColor.value = previousCustom }
    status(`Не удалось сохранить цвет: ${String(error)}`, 'error')
  } finally {
    if (request === appearanceRequest) busy.value = false
  }
}
async function setUseCustomColor(value: boolean) {
  if (busy.value) return
  const previous = useCustomColor.value
  const request = ++appearanceRequest
  useCustomColor.value = value
  busy.value = true
  try {
    const applied = await invoke<FloatingAppearanceDto>('set_floating_use_custom_color', { enabled: value })
    if (request === appearanceRequest) applyAppearance(applied)
  } catch (error) {
    if (request === appearanceRequest) useCustomColor.value = previous
    status(`Не удалось сохранить режим своей темы: ${String(error)}`, 'error')
  } finally {
    if (request === appearanceRequest) busy.value = false
  }
}
onMounted(() => { void loadAppearance() })
</script>

<template>
  <div class="settings-interface">
    <StatusMessage :message="statusMessage" :type="statusType" @dismiss="statusMessage = ''" />
    <section class="settings-section">
      <div class="theme-selector">
        <label class="theme-option" :class="{ active: selectedTheme === 'dark' }"><input v-model="selectedTheme" type="radio" value="dark" @change="setTheme" /><Moon :size="16" /><span>Тёмная</span></label>
        <label class="theme-option" :class="{ active: selectedTheme === 'light' }"><input v-model="selectedTheme" type="radio" value="light" @change="setTheme" /><Sun :size="16" /><span>Светлая</span></label>
      </div>
    </section>
    <section class="settings-section">
      <h2 class="section-title">Плавающее окно</h2>
      <label class="setting-label checkbox-label"><input type="checkbox" class="checkbox-input" :checked="useCustomColor" :disabled="busy" :aria-checked="useCustomColor" role="switch" @change="void setUseCustomColor(($event.target as HTMLInputElement).checked)" /><span>Использовать свою тему</span></label>
      <div class="appearance-grid">
        <div class="appearance-column"><label class="setting-label" for="bg-color">Цвет</label><div class="appearance-controls"><input id="bg-color" type="color" class="color-input" :value="bgColor" :disabled="!useCustomColor || busy" @change="setColor(($event.target as HTMLInputElement).value)" /><input v-model="colorText" type="text" maxlength="7" class="text-input color-text" :disabled="!useCustomColor || busy" @blur="setColor(colorText)" @keyup.enter="setColor(colorText)" /></div></div>
        <div class="appearance-column"><label class="setting-label" for="transparency">Прозрачность: {{ transparency }}%</label><div class="appearance-controls"><input id="transparency" type="range" min="0" max="90" step="1" class="slider-input inline-slider" :value="transparency" :disabled="busy" @change="setTransparency(Number(($event.target as HTMLInputElement).value))" /></div></div>
      </div>
    </section>
  </div>
</template>

<style scoped>
.settings-interface { display: flex; flex-direction: column; gap: 1.5rem; }.settings-section { padding: 12px 16px; background: var(--color-bg-field); border: 1px solid var(--color-border); border-radius: 12px; backdrop-filter: blur(8px); }.section-title { margin: 0 0 1rem; font-size: 1.05rem; color: var(--color-text-primary); }.theme-selector { display: flex; gap: 1rem; }.theme-option { display: flex; align-items: center; gap: .5rem; padding: .5rem 1rem; background: var(--color-bg-field); border: 1px solid var(--color-border); border-radius: 8px; cursor: pointer; user-select: none; transition: all .2s; font-size: .9rem; font-weight: 500; color: var(--color-text-secondary); }.theme-option:hover { background: var(--color-bg-field-hover); border-color: var(--color-border-strong); }.theme-option.active { background: var(--btn-accent-bg); border-color: var(--color-accent); color: var(--color-text-primary); }.theme-option input { display: none; }.appearance-grid { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 2fr); gap: 1rem; }.appearance-column { min-width: 0; }.setting-label { display: flex; align-items: center; gap: .6rem; margin-bottom: .5rem; font-size: .95rem; font-weight: 600; color: var(--color-text-primary); }.appearance-controls, .interval-control { display: flex; gap: .75rem; align-items: center; flex-wrap: wrap; }.color-input { width: 50px; height: 36px; padding: 0; border: 1px solid var(--color-border-strong); border-radius: 10px; background: transparent; cursor: pointer; }.text-input { padding: .6rem; border: 1px solid var(--color-border-strong); border-radius: 10px; background: var(--color-bg-field); color: var(--color-text-primary); }.color-text { width: 95px; font: 14px var(--font-mono); text-transform: uppercase; }.interval-input { width: 96px; }.interval-control { color: var(--color-text-secondary); }.slider-input { width: 100%; accent-color: var(--color-accent); cursor: pointer; }.inline-slider { flex: 1; min-width: 100px; }.setting-row { display: block; margin: 1rem 0; }.message-clear-setting { margin-bottom: 0; }.setting-hint { display: block; margin: .4rem 0 0 2.4rem; font-size: .85rem; color: var(--color-text-muted); line-height: 1.4; }.setting-hint.no-indent { margin-left: 0; }.reset-button { display: inline-flex; align-items: center; gap: .5rem; padding: .5rem .75rem; border: 1px solid var(--color-border-strong); border-radius: 6px; background: var(--color-bg-field-hover); color: var(--color-text-primary); cursor: pointer; }.reset-button:hover { border-color: var(--color-accent); }.reset-button:disabled { opacity: .5; cursor: wait; }
.checkbox-input { appearance: none; width: 38px; height: 22px; flex: 0 0 auto; margin: 0; border: 1px solid var(--color-border-strong); border-radius: 999px; background: var(--color-bg-field-hover); cursor: pointer; position: relative; transition: background .18s ease, border-color .18s ease; }
.checkbox-input::after { content: ''; position: absolute; top: 3px; left: 3px; width: 14px; height: 14px; border-radius: 50%; background: var(--color-text-secondary); transition: transform .18s ease, background .18s ease; }
.checkbox-input:checked { background: var(--color-accent); border-color: var(--color-accent); }
.checkbox-input:checked::after { background: var(--color-text-white); transform: translateX(16px); }
.checkbox-input:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
.checkbox-input:disabled { opacity: .55; cursor: wait; }
</style>
