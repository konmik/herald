import { canAnnounce } from "./work-state"

export async function canAnnounceFromLocalServer(sessionID: string, failed: boolean) {
  const { OpenCode } = await import("@opencode/client")
  const { Service } = await import("@opencode/client/service")
  const endpoint = await Service.discover()
  if (!endpoint) throw new Error("Cannot discover the announcement server without starting a service")
  const client = OpenCode.make({
    baseUrl: endpoint.url,
    headers: Service.headers(endpoint),
  })
  const info = await client.server.info({ signal: AbortSignal.timeout(10_000) })
  if (info.pid !== process.pid) throw new Error("Announcement state belongs to another OpenCode server")
  return canAnnounce(client, sessionID, failed)
}
