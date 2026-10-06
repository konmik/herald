import { Plugin } from "@opencode/plugin/tui"
import { createEffect, createSignal, onCleanup } from "solid-js"
import { randomUUID } from "node:crypto"
import { send } from "./bridge"

export default Plugin.define({
  id: "civilized-agent.tui",
  setup(ctx) {
    const clientID = randomUUID()
    return ctx.ui.slot({
      append: "app",
      render() {
        const [focused, setFocused] = createSignal(true)
        const report = () => {
          const route = ctx.ui.router.current()
          void send({ type: "presence", clientID, sessionID: focused() && route.type === "session" ? route.sessionID : null, at: Date.now() }).catch(console.error)
        }
        const focus = () => setFocused(true)
        const blur = () => setFocused(false)
        ctx.renderer.on("focus", focus)
        ctx.renderer.on("blur", blur)
        createEffect(report)
        const heartbeat = setInterval(report, 2000)
        onCleanup(() => {
          clearInterval(heartbeat)
          ctx.renderer.off("focus", focus)
          ctx.renderer.off("blur", blur)
          void send({ type: "presence", clientID, sessionID: null, at: Date.now() }).catch(console.error)
        })
        return null
      },
    })
  },
})
