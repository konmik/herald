import { appendFile } from "node:fs/promises"
import { Plugin } from "@opencode/plugin"

export default Plugin.define({
  id: "civilized-host-verification",
  async setup(ctx) {
    const proof = process.env.CIVILIZED_AGENT_HOST_PROOF
    const marker = process.env.CIVILIZED_AGENT_HOST_MARKER
    if (!proof || !marker) throw new Error("Host verification requires an evidence path and context marker")
    const write = (value: object) => appendFile(proof, JSON.stringify({ at: Date.now(), ...value }) + "\n")
    await ctx.session.hook("generate", async (event) => {
      const hasPriorContext = JSON.stringify(event.messages).includes(marker)
      await write({ type: "generate", sessionID: event.sessionID, hasPriorContext, messageCount: event.messages.length })
      if (!hasPriorContext) throw new Error("Announcement generation lost the original conversation")
    })
    const controller = new AbortController()
    const events = (async () => {
      for await (const event of ctx.event.subscribe({ signal: controller.signal })) {
        if (event.type.startsWith("session.execution.") || ["session.created", "session.inbox.enqueued", "session.deleted", "shell.created", "shell.exited", "shell.deleted"].includes(event.type)) {
          await write({ type: "event", event })
        }
      }
    })()
    return async () => {
      controller.abort()
      await events
    }
  },
})
