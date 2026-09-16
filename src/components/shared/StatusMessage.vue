<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { Check, AlertCircle, Info, AlertTriangle, X } from 'lucide-vue-next'
import type { MessageType } from '@/types'

interface Props {
  message: string
  type?: MessageType
  autoHide?: boolean
  autoHideDelay?: number
  dismissible?: boolean
}

const props = withDefaults(defineProps<Props>(), {
  type: 'info',
  autoHide: true,
  autoHideDelay: 3000,
  dismissible: true,
})

const emit = defineEmits<{
  (e: 'dismiss'): void
}>()

const visible = ref(false)
let timeoutId: ReturnType<typeof setTimeout> | null = null

const iconComponent = computed(() => {
  switch (props.type) {
    case 'success':
      return Check
    case 'error':
      return AlertCircle
    case 'warning':
      return AlertTriangle
    case 'info':
    default:
      return Info
  }
})

function show() {
  visible.value = true
  scheduleAutoHide()
}

function hide() {
  visible.value = false
  if (timeoutId) {
    clearTimeout(timeoutId)
    timeoutId = null
  }
}

function scheduleAutoHide() {
  if (timeoutId) {
    clearTimeout(timeoutId)
  }

  if (props.autoHide && props.message) {
    timeoutId = setTimeout(() => {
      emit('dismiss')
    }, props.autoHideDelay)
  }
}

function onDismiss() {
  emit('dismiss')
}

watch(() => props.message, () => {
  if (props.message) {
    show()
  } else {
    hide()
  }
})

onMounted(() => {
  if (props.message) {
    show()
  }
})

onUnmounted(() => {
  if (timeoutId) {
    clearTimeout(timeoutId)
  }
})
</script>

<template>
  <Transition name="fade-slide">
    <div v-if="visible" class="status-message" :class="type">
      <component :is="iconComponent" :size="16" class="status-icon" />
      <span class="status-text">{{ message }}</span>
      <button
        v-if="dismissible"
        class="status-close"
        aria-label="Закрыть уведомление"
        title="Закрыть"
        @click="onDismiss"
        type="button"
      >
        <X :size="14" />
      </button>
    </div>
  </Transition>
</template>

<style scoped>
.status-message {
  position: fixed;
  top: 20px;
  left: calc(50% + 100px);
  transform: translateX(-50%);
  padding: 0.5rem 0.75rem;
  border-radius: 8px;
  z-index: 1000;
  backdrop-filter: blur(10px);
  display: flex;
  align-items: center;
  gap: 0.5rem;
  min-width: 280px;
  max-width: 420px;
  box-shadow: var(--shadow-soft);
}

.status-icon {
  flex-shrink: 0;
}

.status-text {
  flex: 1;
  font-size: 0.875rem;
  line-height: 1.4;
}

.status-close {
  flex-shrink: 0;
  padding: 2px;
  border-radius: 4px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  color: inherit;
  opacity: 0.7;
  transition: opacity 0.15s ease;
}

.status-close:hover {
  opacity: 1;
}

/* Type variants */
.status-message.success {
  background: var(--success-bg);
  border: 1px solid var(--success-border);
  color: var(--success-text);
}

.status-message.error {
  background: var(--danger-bg);
  border: 1px solid var(--danger-border);
  color: var(--danger-text);
}

.status-message.warning {
  background: var(--warning-bg);
  border: 1px solid var(--warning-border);
  color: var(--warning-text);
}

.status-message.info {
  background: var(--info-bg);
  border: 1px solid var(--info-border);
  color: var(--info-text);
}

/* Transitions */
.fade-slide-enter-active {
  transition: all 0.28s ease;
}

.fade-slide-leave-active {
  transition: all 0.2s ease;
}

.fade-slide-enter-from {
  opacity: 0;
  transform: translate(-50%, -10px);
}

.fade-slide-leave-to {
  opacity: 0;
  transform: translate(-50%, -10px);
}

.fade-slide-enter-to,
.fade-slide-leave-from {
  opacity: 1;
  transform: translate(-50%, 0);
}
</style>
