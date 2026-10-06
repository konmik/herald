import { appendFileSync, readFileSync } from 'node:fs'

if (!process.env.CIVILIZED_AGENT_HOST_PROOF) throw new Error('Missing host evidence path')
const value = JSON.parse(readFileSync(0, 'utf8'))
appendFileSync(process.env.CIVILIZED_AGENT_HOST_PROOF, JSON.stringify(value) + '\n')
