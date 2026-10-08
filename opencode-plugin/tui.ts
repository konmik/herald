import type { Plugin } from "@opencode/plugin/tui"
import { randomUUID } from "node:crypto"
import { send } from "./bridge"

export default {
  id: "herald.tui",
  setup(ctx) {
    const clientID = randomUUID()
    let sequence = 0
    const report = () => {
      const route = ctx.ui.router.current()
      const sessionIDs = ctx.ui.tabs.enabled()
        ? ctx.ui.tabs.list().map((tab) => tab.sessionID)
        : route.type === "session" ? [ctx.data.session.root(route.sessionID) ?? route.sessionID] : []
      void send({ type: "presence", clientID, sessionIDs, sequence: ++sequence, at: Date.now() }).catch(console.error)
    }
    report()
    const heartbeat = setInterval(report, 2000)
    return async () => {
      clearInterval(heartbeat)
      await send({ type: "presence", clientID, sessionIDs: [], sequence: ++sequence, at: Date.now() }).catch(console.error)
    }
  },
} satisfies Plugin.Definition
