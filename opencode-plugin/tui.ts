import { Plugin } from "@opencode/plugin/tui"
import { createEffect, onCleanup } from "solid-js"
import { randomUUID } from "node:crypto"
import { send } from "./bridge"

export default Plugin.define({
  id: "civilized-agent.tui",
  setup(ctx) {
    const clientID = randomUUID()
    let sequence = 0
    return ctx.ui.slot({
      append: "app",
      render() {
        const report = () => {
          const route = ctx.ui.router.current()
          const sessionIDs = ctx.ui.tabs.enabled()
            ? ctx.ui.tabs.list().map((tab) => tab.sessionID)
            : route.type === "session" ? [ctx.data.session.root(route.sessionID) ?? route.sessionID] : []
          void send({ type: "presence", clientID, sessionIDs, sequence: ++sequence, at: Date.now() }).catch(console.error)
        }
        createEffect(report)
        const heartbeat = setInterval(report, 2000)
        onCleanup(() => {
          clearInterval(heartbeat)
          void send({ type: "presence", clientID, sessionIDs: [], sequence: ++sequence, at: Date.now() }).catch(console.error)
        })
        return null
      },
    })
  },
})
