import type { OpenCode } from "@opencode/client"

export async function ready(ctx: Pick<ReturnType<typeof OpenCode.make>, "session" | "shell">, sessionID: string, failed: boolean) {
  const options = { signal: AbortSignal.timeout(10_000) }
  const root = await ctx.session.get({ sessionID }, options)
  if (root.parentID || root.outcome !== (failed ? "failed" : "succeeded")) return false
  const sessions = [root]
  const visited = new Set([sessionID])
  for (const parent of sessions) {
    let cursor: string | undefined
    do {
      const page = await ctx.session.list({ parentID: parent.id, limit: 100, cursor }, options)
      for (const child of page.data) {
        if (visited.has(child.id)) continue
        visited.add(child.id)
        sessions.push(child)
      }
      cursor = page.cursor.next ?? undefined
    } while (cursor)
  }
  const active = await ctx.session.active(options)
  if (sessions.some((session) => session.id in active)) return false
  for (const session of sessions) {
    if ((await ctx.session.inbox.list({ sessionID: session.id }, options)).length) return false
  }
  const directories = new Set(sessions.map((session) => session.location.directory))
  for (const directory of directories) {
    const shells = await ctx.shell.list({ location: { directory } }, options)
    if (shells.data.some((shell) => shell.status === "running" && typeof shell.metadata.sessionID === "string" && visited.has(shell.metadata.sessionID))) return false
  }
  if (!failed) {
    const messages = await ctx.session.context({ sessionID }, options)
    const assistant = messages.findLast((message) => message.type === "assistant")
    if (!assistant || assistant.type !== "assistant" || assistant.finish !== "stop" || !assistant.content.some((part) => part.type === "text" && part.text.trim())) return false
  }
  const latest = await ctx.session.active(options)
  return !sessions.some((session) => session.id in latest)
}
