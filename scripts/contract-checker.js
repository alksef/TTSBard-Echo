#!/usr/bin/env node

import fs from 'node:fs'
import path from 'node:path'

const root = process.cwd()
const read = file => fs.readFileSync(path.join(root, file), 'utf8')
const fail = message => { console.error(`contract-check: ${message}`); process.exitCode = 1 }
const assert = (condition, message) => { if (!condition) fail(message) }

function registeredCommands() {
  const lib = read('src-tauri/src/lib.rs')
  return new Set([...lib.matchAll(/commands::(?:app|connections|settings|windows)::([a-z0-9_]+)/g)].map(match => match[1]))
}

function invokedCommands() {
  const source = fs.readdirSync(path.join(root, 'src'), { recursive: true })
    .filter(file => String(file).endsWith('.ts') || String(file).endsWith('.vue'))
    .map(file => read(path.join('src', file)))
    .join('\n')
  return new Set([...source.matchAll(/invoke(?:<[^>]+>)?\(\s*['"]([a-z0-9_]+)['"]/g)].map(match => match[1]))
}

function checkIpc() {
  const registered = registeredCommands()
  const invoked = invokedCommands()
  for (const command of invoked) assert(registered.has(command), `frontend invokes unregistered command: ${command}`)
  assert(registered.has('get_connection_runtime_snapshot'), 'runtime snapshot command is not registered')
  assert(registered.has('reset_floating_window_position'), 'reset position command is not registered')
  console.log(`check:ipc ok (${registered.size} registered, ${invoked.size} frontend invokes)`)
}

function checkSettings() {
  const dto = read('src-tauri/src/config/dto.rs') + read('src-tauri/src/config/settings.rs')
  const types = read('src/types/settings.ts')
  const fields = [
    ['LoggingSettingsDto', ['enabled', 'level', 'module_levels']],
    ['GeneralSettingsDto', ['exclude_from_capture', 'hide_on_minimize', 'theme', 'message_clear_interval_seconds']],
    ['ConnectionConfig', ['id', 'name', 'url', 'enabled', 'access_token']],
    ['FloatingWindowDto', ['x', 'y', 'opacity', 'bg_color', 'clickthrough']],
  ]
  for (const [name, names] of fields) {
    assert(dto.includes(`struct ${name}`) || dto.includes(`pub struct ${name}`), `Rust DTO missing ${name}`)
    for (const field of names) assert(types.includes(field), `TypeScript settings missing ${name}.${field}`)
  }
  assert(types.includes('opacityToTransparency') && types.includes('transparencyToOpacity'), 'opacity conversion helpers missing')
  console.log('check:settings ok (DTO/type fields and appearance helpers)')
}

const mode = process.argv[2] ?? 'all'
if (mode === 'ipc' || mode === 'all') checkIpc()
if (mode === 'settings' || mode === 'all') checkSettings()
if (!['ipc', 'settings', 'all'].includes(mode)) { fail(`unknown mode: ${mode}`) }
if (process.exitCode) process.exit(process.exitCode)
