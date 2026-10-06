import { createServer } from 'node:http'
import { appendFile, writeFile } from 'node:fs/promises'

const [addressFile, requestsFile] = process.argv.slice(2)
if (!addressFile || !requestsFile) throw new Error('Provide address and request log paths.')
const pcm = Buffer.alloc(16000)
for (let index = 0; index < pcm.length / 2; index++) pcm.writeInt16LE(Math.round(3000 * Math.sin(2 * Math.PI * 440 * index / 16000)), index * 2)
const server = createServer(async (request, response) => {
  try {
    const chunks = []
    for await (const chunk of request) chunks.push(chunk)
    const body = JSON.parse(Buffer.concat(chunks).toString() || '{}')
    await appendFile(requestsFile, `${JSON.stringify({ method: request.method, url: request.url, body })}\n`)
    if (request.headers['xi-api-key'] !== 'character-ui-test-key') {
      response.writeHead(401, { 'Content-Type': 'application/json' })
      response.end(JSON.stringify({ detail: 'Invalid test key' }))
      return
    }
    if (request.method !== 'POST') {
      response.writeHead(405).end()
      return
    }
    const url = new URL(request.url, 'http://localhost')
    if (url.pathname === '/v1/text-to-voice/design') {
      response.writeHead(200, { 'Content-Type': 'application/json' })
      response.end(JSON.stringify({ previews: [0, 1, 2].map(index => ({
        audio_base_64: pcm.toString('base64'),
        generated_voice_id: `generated-${index}`,
        duration_secs: 0.5,
        media_type: 'audio/pcm',
      })) }))
    } else if (url.pathname === '/v1/text-to-voice') {
      response.writeHead(200, { 'Content-Type': 'application/json' })
      response.end(JSON.stringify({ voice_id: `saved-${body.generated_voice_id}` }))
    } else if (url.pathname.startsWith('/v1/text-to-speech/')) {
      response.writeHead(200, { 'Content-Type': 'audio/pcm' })
      response.end(pcm)
    } else {
      response.writeHead(404).end()
    }
  } catch (error) {
    response.writeHead(500, { 'Content-Type': 'application/json' })
    response.end(JSON.stringify({ detail: error.message }))
  }
})
server.listen(0, '127.0.0.1', async () => {
  await writeFile(addressFile, `http://127.0.0.1:${server.address().port}`)
})
