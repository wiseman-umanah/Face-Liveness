// Serves the demo and runs the authoritative liveness check on the server.
//
//   node demo/server.mjs            then open http://localhost:8080
//
// Flow: POST /api/session issues a signed ticket and starts an engine for it; the browser posts
// frames to /api/session/:nonce/frame; when the check ends the response carries a signed
// receipt; POST /api/verify checks a receipt the way another service would (signature, expiry,
// and each nonce only once). Timing uses the server's clock, not the client's.
//
// Uses the `faceliveness` npm package built from crates/liveness-node (pnpm build there first);
// in your own app: import { Engine, SigningKey, verifyReceipt, verifyTicket } from 'faceliveness'

import { readFile } from 'node:fs/promises'
import { createServer } from 'node:http'
import { extname, join, resolve, sep } from 'node:path'

import { Engine, SigningKey, verifyReceipt, verifyTicket } from '../crates/liveness-node/index.js'

const PORT = Number(process.env.PORT ?? 8080)
const HOST = process.env.HOST ?? '127.0.0.1'
const ROOT = resolve(import.meta.dirname)
const MAX_SESSIONS = 20
const MAX_FRAME_SIDE = 1280
const SESSION_TTL_MS = 3 * 60_000

// In production load a persistent key from your secret store; LIVENESS_KEY is 64 hex chars.
const key = process.env.LIVENESS_KEY
  ? SigningKey.fromBytes(Buffer.from(process.env.LIVENESS_KEY, 'hex'))
  : SigningKey.generate()

const sessions = new Map() // nonce -> { engine, startedAt, busy, final }
const usedNonces = new Set() // receipts accepted by /api/verify; use a database in production

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.wasm': 'application/wasm',
  '.json': 'application/json',
}

function send(res, status, body) {
  const headers = { 'content-type': 'application/json' }
  if (!res.req.complete) {
    // Only for uploads over the size cap: stop reading and drop the connection.
    headers.connection = 'close'
    res.on('finish', () => res.req.socket.destroy())
  }
  res.writeHead(status, headers)
  res.end(JSON.stringify(body))
}

async function readBody(req, limit) {
  const chunks = []
  let size = 0
  for await (const chunk of req) {
    size += chunk.length
    if (size > limit) throw Object.assign(new Error('body too large'), { status: 413 })
    chunks.push(chunk)
  }
  return Buffer.concat(chunks)
}

function publicStatus(r) {
  return {
    status: r.status,
    done: r.done,
    currentChallenge: r.currentChallenge,
    remaining: r.remaining,
    failReason: r.failReason,
    actions: r.actions,
  }
}

async function newSession(res) {
  if (sessions.size >= MAX_SESSIONS) return send(res, 503, { error: 'too many sessions, try again shortly' })
  const ticket = key.issueTicket({ count: 3, ttlSecs: 120 })
  const { nonce, challenges, expiresAt } = verifyTicket(ticket, key.publicKey)
  sessions.set(nonce, {
    engine: Engine.fromTicket(ticket, key.publicKey),
    startedAt: Date.now(),
    busy: false,
    final: null,
  })
  send(res, 200, { nonce, challenges, expiresAt })
}

async function frame(req, res, nonce, url) {
  // Read the upload before any check, so rejections don't leave unread bytes on the connection.
  const pixels = await readBody(req, MAX_FRAME_SIDE * MAX_FRAME_SIDE * 4)
  const session = sessions.get(nonce)
  if (!session) return send(res, 404, { error: 'unknown or expired session' })
  if (session.final) return send(res, 200, session.final)
  if (session.busy) return send(res, 429, { error: 'send one frame at a time' })

  const width = Number(url.searchParams.get('w'))
  const height = Number(url.searchParams.get('h'))
  if (!(width > 0 && height > 0 && width <= MAX_FRAME_SIDE && height <= MAX_FRAME_SIDE)) {
    return send(res, 400, { error: 'bad frame size' })
  }
  if (pixels.length !== width * height * 4) return send(res, 400, { error: 'expected RGBA pixels' })

  session.busy = true
  try {
    const result = await session.engine.processAsync(pixels, {
      width,
      height,
      format: 'rgba',
      timestampMs: Date.now() - session.startedAt, // server clock: the client can't stretch time
    })
    if (result.done) {
      session.final = { ...publicStatus(result), receipt: session.engine.receipt(key) }
      return send(res, 200, session.final)
    }
    send(res, 200, publicStatus(result))
  } finally {
    session.busy = false
  }
}

async function verify(req, res) {
  const token = (await readBody(req, 8192)).toString('utf8').trim()
  let receipt
  try {
    receipt = verifyReceipt(token, key.publicKey)
  } catch (error) {
    return send(res, 200, { valid: false, error: error.message })
  }
  if (usedNonces.has(receipt.nonce)) return send(res, 200, { valid: false, error: 'receipt already used' })
  usedNonces.add(receipt.nonce)
  send(res, 200, { valid: true, ...receipt })
}

async function serveFile(res, pathname) {
  const file = resolve(ROOT, `.${pathname === '/' ? '/index.html' : pathname}`)
  if (!file.startsWith(ROOT + sep) || file.endsWith('.mjs')) return send(res, 404, { error: 'not found' })
  try {
    const body = await readFile(file)
    res.writeHead(200, { 'content-type': MIME[extname(file)] ?? 'application/octet-stream' })
    res.end(body)
  } catch {
    send(res, 404, { error: 'not found' })
  }
}

const server = createServer(async (req, res) => {
  const url = new URL(req.url, 'http://localhost')
  try {
    const frameMatch = url.pathname.match(/^\/api\/session\/([0-9a-f]{32})\/frame$/)
    if (req.method === 'GET' && url.pathname === '/api/health') return send(res, 200, { ok: true })
    if (req.method === 'POST' && url.pathname === '/api/session') return await newSession(res)
    if (req.method === 'POST' && frameMatch) return await frame(req, res, frameMatch[1], url)
    if (req.method === 'POST' && url.pathname === '/api/verify') return await verify(req, res)
    if (req.method === 'GET') return await serveFile(res, url.pathname)
    send(res, 405, { error: 'method not allowed' })
  } catch (error) {
    send(res, error.status ?? 500, { error: error.message })
  }
})

// Drop sessions nobody finished.
setInterval(() => {
  for (const [nonce, s] of sessions) if (Date.now() - s.startedAt > SESSION_TTL_MS) sessions.delete(nonce)
}, 30_000).unref()

server.listen(PORT, HOST, () => {
  console.log(`Face Liveness demo on http://${HOST === '127.0.0.1' ? 'localhost' : HOST}:${PORT}`)
  console.log(`receipt public key: ${key.publicKey.toString('hex')}`)
})
