#!/usr/bin/env node

import fs from 'node:fs'
import path from 'node:path'

const root = process.cwd()
const read = file => fs.readFileSync(path.join(root, file), 'utf8')
let issueCount = 0
const fail = message => { issueCount += 1; console.error(`contract-check: ${message}`); process.exitCode = 1 }
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
  const issuesBefore = issueCount
  const registered = registeredCommands()
  const invoked = invokedCommands()
  for (const command of invoked) assert(registered.has(command), `frontend invokes unregistered command: ${command}`)
  assert(registered.has('get_connection_runtime_snapshot'), 'runtime snapshot command is not registered')
  assert(registered.has('reset_floating_window_position'), 'reset position command is not registered')
  if (issueCount === issuesBefore) console.log(`check:ipc ok (${registered.size} registered, ${invoked.size} frontend invokes)`)
}

function checkSettings() {
  const issuesBefore = issueCount
  const rust = read('src-tauri/src/config/dto.rs') + '\n' + read('src-tauri/src/config/settings.rs')
  const types = read('src/types/settings.ts')
  const fields = [
    ['LoggingSettingsDto', 'LoggingSettingsDto', ['enabled', 'level', 'module_levels']],
    ['GeneralSettingsDto', 'GeneralSettingsDto', ['exclude_from_capture', 'hide_on_minimize', 'theme', 'message_clear_interval_seconds']],
    ['ConnectionConfig', 'ConnectionConfig', ['id', 'name', 'url', 'enabled', 'access_token']],
    ['FloatingWindowDto', 'FloatingWindowSettingsDto', ['x', 'y', 'opacity', 'bg_color', 'clickthrough', 'use_custom_color', 'visible']],
  ]

  const block = (source, pattern, label) => {
    const match = pattern.exec(source)
    assert(match, `missing ${label}`)
    if (!match) return ''
    let depth = 1
    const start = match.index + match[0].length
    for (let end = start; end < source.length; end += 1) {
      if (source[end] === '{') depth += 1
      if (source[end] === '}') depth -= 1
      if (depth === 0) return source.slice(start, end)
    }
    fail(`unterminated ${label}`)
    return ''
  }

  for (const [rustName, tsName, names] of fields) {
    const rustBody = block(rust, new RegExp(`(?:pub\\s+)?struct\\s+${rustName}\\s*\\{`, 'm'), `Rust ${rustName}`)
    const tsBody = block(types, new RegExp(`export\\s+interface\\s+${tsName}\\s*\\{`, 'm'), `TypeScript ${tsName}`)
    const rustFields = new Set([...rustBody.matchAll(/^\s*pub\s+([A-Za-z0-9_]+)\s*:/gm)].map(match => match[1]))
    const tsFields = new Set([...tsBody.matchAll(/^\s*([A-Za-z0-9_]+)\??\s*:/gm)].map(match => match[1]))
    for (const field of names) {
      assert(rustFields.has(field), `Rust settings missing ${rustName}.${field}`)
      assert(tsFields.has(field), `TypeScript settings missing ${tsName}.${field}`)
    }
  }
  assert(types.includes('opacityToTransparency') && types.includes('transparencyToOpacity'), 'opacity conversion helpers missing')
  if (issueCount === issuesBefore) console.log('check:settings ok (Rust/TypeScript DTO fields and appearance helpers)')
}

const mode = process.argv[2] ?? 'all'
if (mode === 'ipc' || mode === 'all') checkIpc()
if (mode === 'settings' || mode === 'all') checkSettings()
if (!['ipc', 'settings', 'all'].includes(mode)) { fail(`unknown mode: ${mode}`) }
if (process.exitCode) process.exit(process.exitCode)
