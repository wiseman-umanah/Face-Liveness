// Face Liveness demo: camera -> Web Worker (WebAssembly engine) -> prompts and landmarks.
// When served by server.mjs, "server-verified" mode also streams frames to the server, which
// runs the authoritative check and returns a signed receipt.

const FRAME_WIDTH = 320
const FRAME_HEIGHT = 240
const PROMPTS = {
  blink: 'Blink',
  turn_left: 'Turn your head to your left',
  turn_right: 'Turn your head to your right',
  open_mouth: 'Open your mouth wide',
}
const FAILURES = {
  timeout: 'The check took too long.',
  step_timeout: "That step wasn't completed in time.",
  face_lost: 'Your face left the camera view.',
  multiple_faces: 'More than one face was in view.',
  spoof_suspected: 'The image looked like a photo or screen.',
}

const $ = (id) => document.getElementById(id)
const params = new URLSearchParams(location.search)
const worker = new Worker(new URL('./worker.js', import.meta.url), { type: 'module' })
const capture = new OffscreenCanvas(FRAME_WIDTH, FRAME_HEIGHT)
const captureCtx = capture.getContext('2d', { willReadFrequently: true })
const overlay = $('overlay')
const overlayCtx = overlay.getContext('2d')
overlay.width = FRAME_WIDTH
overlay.height = FRAME_HEIGHT

let serverAvailable = false
let run = null // the current attempt

function log(event, data) {
  // One JSON line per event, for automated end-to-end tests.
  console.log(`LIVENESS ${JSON.stringify({ event, ...data })}`)
}

function setPrompt(text) {
  $('prompt').textContent = text
}

function renderSteps(challenges, remaining, done) {
  const current = challenges.length - remaining
  $('steps').replaceChildren(
    ...challenges.map((c, i) => {
      const li = document.createElement('li')
      li.textContent = PROMPTS[c]
      if (done || i < current) li.className = 'done'
      else if (i === current) li.className = 'current'
      return li
    }),
  )
}

function drawLandmarks(points) {
  overlayCtx.clearRect(0, 0, FRAME_WIDTH, FRAME_HEIGHT)
  if (!points) return
  overlayCtx.fillStyle = 'rgba(79, 140, 255, 0.8)'
  for (const [x, y] of points) overlayCtx.fillRect(x - 0.5, y - 0.5, 1, 1)
}

function grabFrame() {
  captureCtx.drawImage($('video'), 0, 0, FRAME_WIDTH, FRAME_HEIGHT)
  return captureCtx.getImageData(0, 0, FRAME_WIDTH, FRAME_HEIGHT).data.buffer
}

// --- worker ---------------------------------------------------------------

const pending = new Map()
worker.onmessage = ({ data }) => {
  if (data.type === 'error') {
    log('error', { message: data.message })
    setPrompt(`Error: ${data.message}`)
  }
  pending.get(data.type)?.(data)
}
const ask = (message, reply) =>
  new Promise((resolve) => {
    pending.set(reply, resolve)
    worker.postMessage(message, message.pixels ? [message.pixels] : [])
  })

// --- server ---------------------------------------------------------------

async function detectServer() {
  try {
    const res = await fetch('api/health')
    return res.ok && (await res.json()).ok === true
  } catch {
    return false
  }
}

async function sendToServer(pixels) {
  const res = await fetch(`api/session/${run.nonce}/frame?w=${FRAME_WIDTH}&h=${FRAME_HEIGHT}`, {
    method: 'POST',
    headers: { 'content-type': 'application/octet-stream' },
    body: pixels,
  })
  if (!res.ok) throw new Error(`server: ${(await res.json()).error}`)
  return res.json()
}

// --- the check ------------------------------------------------------------

async function start() {
  $('start').disabled = true
  $('result').hidden = true
  const serverMode = serverAvailable && $('server-mode').checked
  $('mode').textContent = serverMode ? 'Server-verified' : 'On-device result (not tamper-proof)'

  const stream = await navigator.mediaDevices.getUserMedia({
    video: { width: 640, height: 480, facingMode: 'user' },
    audio: false,
  })
  const video = $('video')
  video.srcObject = stream
  await video.play()

  run = { serverMode, stream, frames: 0, startedAt: performance.now(), done: false }
  setPrompt('Loading face models…')
  if (serverMode) {
    const session = await (await fetch('api/session', { method: 'POST' })).json()
    Object.assign(run, { nonce: session.nonce, challenges: session.challenges })
    // The local engine only draws landmarks here; the server decides, so it never times out.
    await ask({ type: 'start', challenges: session.challenges, options: { totalTimeoutMs: 1e12, stepTimeoutMs: 1e12, faceLostMs: 1e12 } }, 'started')
  } else {
    run.challenges = (await ask({ type: 'start', count: 3 }, 'started')).challenges
  }
  log('started', { challenges: run.challenges, serverMode })
  renderSteps(run.challenges, run.challenges.length, false)
  setPrompt(PROMPTS[run.challenges[0]])
  run.startedAt = performance.now()
  localLoop()
  if (serverMode) serverLoop()
}

async function localLoop() {
  while (run && !run.done) {
    const pixels = grabFrame()
    const { result, ms } = await ask(
      { type: 'frame', pixels, width: FRAME_WIDTH, height: FRAME_HEIGHT, timestampMs: Math.round(performance.now() - run.startedAt) },
      'result',
    )
    if (!run || run.done) break
    run.frames++
    drawLandmarks(result.landmarks)
    showStats(result, ms)
    if (!run.serverMode) applyStatus(result)
  }
}

async function serverLoop() {
  while (run && !run.done) {
    let status
    try {
      status = await sendToServer(grabFrame())
    } catch (error) {
      return finish({ status: 'failed', failReason: null, error: String(error.message) })
    }
    applyStatus(status)
  }
}

function showStats(result, ms) {
  const s = result.signals
  const parts = [`${(1000 / ms).toFixed(0)} fps (${ms.toFixed(0)} ms/frame)`]
  if (s) parts.push(`eyes ${s.ear.toFixed(2)}`, `mouth ${s.mouth.toFixed(2)}`, `turn ${s.yaw.toFixed(2)}`)
  if (result.realScore != null) run.realScore = result.realScore
  if (run.realScore != null) parts.push(`real ${run.realScore.toFixed(2)}`)
  if (result.faceCount === 0) parts.push('no face')
  $('stats').textContent = parts.join('  ·  ')
}

function applyStatus(status) {
  for (const action of status.actions ?? []) log('action', { action })
  if (status.done) return finish(status)
  renderSteps(run.challenges, status.remaining, false)
  setPrompt(PROMPTS[status.currentChallenge])
}

async function finish(status) {
  if (run.done) return
  run.done = true
  run.stream.getTracks().forEach((t) => t.stop())
  drawLandmarks(null)
  const passed = status.status === 'passed'
  renderSteps(run.challenges, 0, passed)
  setPrompt(passed ? 'Done' : 'Try again')
  $('verdict').textContent = passed ? 'Liveness check passed' : 'Liveness check failed'
  $('verdict-detail').textContent = passed
    ? run.serverMode
      ? 'The server ran the check on your frames and signed this result.'
      : 'Computed on your device. A server should re-check frames before trusting it.'
    : (status.error ?? FAILURES[status.failReason] ?? 'Something went wrong.')
  $('result').className = `result ${passed ? 'passed' : 'failed'}`
  $('result').hidden = false

  if (status.receipt) {
    // A separate endpoint verifies the signed receipt, as another service would.
    const verified = await (await fetch('api/verify', { method: 'POST', body: status.receipt })).json()
    $('receipt').textContent = `${status.receipt}\n\nVerified by /api/verify:\n${JSON.stringify(verified, null, 2)}`
    $('receipt-box').hidden = false
    log('receipt', { verified })
  }
  log('finished', { status: status.status, failReason: status.failReason ?? null, frames: run.frames })
  $('start').disabled = false
  $('start').textContent = 'Try again'
}

// --- boot -----------------------------------------------------------------

;(async () => {
  const { ms } = await ask({ type: 'init' }, 'ready')
  log('ready', { ms: Math.round(ms) })
  serverAvailable = await detectServer()
  $('server-toggle').hidden = !serverAvailable
  $('mode').textContent = serverAvailable ? '' : 'On-device mode'
  $('start').textContent = 'Start'
  $('start').disabled = false
  $('start').onclick = () =>
    start().catch((error) => {
      log('error', { message: String(error?.message ?? error) })
      setPrompt(`Error: ${error?.message ?? error}`)
      $('start').disabled = false
    })
  if (params.has('autostart')) $('start').click()
})()
