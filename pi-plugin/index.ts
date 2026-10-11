import { SessionManager, convertToLlm, type ExtensionAPI } from '@earendil-works/pi-coding-agent'
import { createHerald } from './herald.mjs'

export default function herald(pi: ExtensionAPI) {
  const runtime = createHerald({ inMemory: SessionManager.inMemory, convertToLlm })
  pi.on('session_start', (_event, ctx) => runtime.open(ctx))
  pi.on('input', (event, ctx) => runtime.input(event, ctx))
  pi.on('before_agent_start', (_event, ctx) => runtime.beforeStart(ctx))
  pi.on('agent_start', (_event, ctx) => runtime.agentStart(ctx))
  pi.on('message_start', (event, ctx) => runtime.messageStart(event, ctx))
  pi.on('agent_before_settle', (event, ctx) => runtime.beforeSettle(event, ctx))
  pi.on('agent_settled', (_event, ctx) => runtime.settled(ctx))
  pi.on('session_before_switch', (_event, ctx) => runtime.navigate(ctx))
  pi.on('session_before_fork', (_event, ctx) => runtime.navigate(ctx))
  pi.on('session_before_tree', (_event, ctx) => runtime.navigate(ctx))
  pi.on('session_tree', (_event, ctx) => runtime.navigate(ctx))
  pi.on('session_shutdown', () => runtime.close())
}
