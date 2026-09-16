<script setup lang="ts">
import type { ConnectionConfig, ConnectionRuntimeSnapshot } from '@/types/settings'
import { connectionStatusLabel } from '@/lib/connectionStatusLabels'
interface ConnectionView extends ConnectionConfig { runtime: ConnectionRuntimeSnapshot }
defineProps<{ connections: ConnectionView[]; loading: boolean; error: string | null }>()
</script>
<template>
  <main class="connection-list" aria-live="polite">
    <p v-if="loading" class="state">Загрузка подключений…</p>
    <p v-else-if="error" class="state error">{{ error }}</p>
    <p v-else-if="!connections.length" class="state">Нет активных подключений</p>
    <ul v-else>
      <li v-for="connection in connections" :key="connection.id" class="connection-card">
        <div class="connection-heading">
          <strong :title="connection.name">{{ connection.name }}</strong>
          <span class="status" :data-status="connection.runtime.status" role="img" :aria-label="`Статус: ${connection.runtime.status}`" :title="connection.runtime.status"></span>
        </div>
        <p v-if="connection.runtime.status === 'Error'" class="error">{{ connectionStatusLabel(connection.runtime) }}</p>
        <p v-else-if="connection.runtime.isTyping" class="typing" aria-label="Набирает">
          <span class="typing-dots" aria-hidden="true"><i>.</i><i>.</i><i>.</i></span>
        </p>
        <p v-else-if="connection.runtime.lastMessage" class="message" :title="connection.runtime.lastMessage">{{ connection.runtime.lastMessage }}</p>
      </li>
    </ul>
  </main>
</template>
<style scoped>
.connection-list { min-height: 0; padding: .5rem; overflow: visible; }
.state { margin: 0; padding: .85rem .6rem; color: var(--color-text-muted); text-align: center; }
.state.error, .error { color: var(--color-danger); }
ul { display: grid; gap: .45rem; margin: 0; padding: 0; list-style: none; }
.connection-card { position: relative; min-height: 2.2rem; padding: .55rem .7rem; border: 1px solid var(--color-border); border-radius: 8px; background: var(--color-bg-field); box-sizing: border-box; }
.connection-heading { display: flex; align-items: center; justify-content: space-between; gap: .5rem; }
.connection-heading strong { min-width: 0; overflow: hidden; color: var(--color-text-muted); font-size: .68rem; font-weight: 600; letter-spacing: .02em; text-overflow: ellipsis; white-space: nowrap; }
.status { flex: 0 0 auto; width: .42rem; height: .42rem; border-radius: 50%; background: var(--color-text-muted); opacity: .55; }
.status[data-status='Connected'] { background: var(--color-success); }
.status[data-status='Connecting'] { background: var(--color-warning); }
.status[data-status^='Error'] { background: var(--color-danger); }
.message, .error { margin: .25rem 0 0; color: var(--color-text-primary); font-size: 1rem; line-height: 1.35; white-space: pre-wrap; overflow-wrap: anywhere; }
.typing { position: absolute; inset: 0; display: flex; align-items: center; justify-content: center; margin: 0; color: var(--color-text-secondary); font-size: 1.5rem; line-height: 1; white-space: pre-wrap; overflow-wrap: anywhere; pointer-events: none; }
.typing-dots { display: inline-flex; width: 1.75rem; justify-content: flex-start; letter-spacing: .12rem; }
.typing-dots i { font-style: normal; animation: typing-dot 1.1s infinite ease-in-out; }
.typing-dots i:nth-child(2) { animation-delay: .16s; }
.typing-dots i:nth-child(3) { animation-delay: .32s; }
@keyframes typing-dot { 0%, 60%, 100% { opacity: .25; transform: translateY(0); } 30% { opacity: 1; transform: translateY(-.08rem); } }
@media (prefers-reduced-motion: reduce) { .typing-dots i { animation: none; opacity: .8; } }
</style>
