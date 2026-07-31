// The live player, as a self-contained HTML document hosted in a WebView.
//
// React Native has no `MediaSource`, and the gateway speaks fragmented MP4 over a WebSocket
// the *browser* engine it already ships: the same MSE state machine the web Live tab runs
// (`sentinel-app/src/routes/live/+page.svelte`), inside a WebView.
//
// The token rides the WS query string because browsers cannot set headers on a WebSocket — the
// gateway accepts it there for exactly this reason.

export type PlayerConfig = {
  wsBase: string;
  stream: string;
  token: string;
};

/** Messages the page posts back to React Native, so the UI can show honest state. */
export type PlayerEvent =
  | { type: "phase"; phase: "connecting" | "live" | "stalled" | "error" }
  | { type: "error"; message: string }
  /** Step-by-step trace. The failure modes here are silent by nature — a socket that opens but
   *  never delivers looks identical to one still connecting — so the player narrates itself. */
  | { type: "log"; message: string };

export function playerHtml({ wsBase, stream, token }: PlayerConfig): string {
  const url = `${wsBase}/api/ws?src=${encodeURIComponent(stream)}&token=${encodeURIComponent(token)}`;
  return `<!doctype html>
<meta name="viewport" content="width=device-width, initial-scale=1, user-scalable=no">
<style>
  html,body{margin:0;height:100%;background:#000;overflow:hidden}
  video{width:100%;height:100%;object-fit:contain;background:#000}
</style>
<video id="v" autoplay muted playsinline></video>
<script>
(function () {
  var v = document.getElementById('v');
  var ws = null, ms = null, sb = null, queue = [], gotBinary = false;

  function post(msg) {
    if (window.ReactNativeWebView) window.ReactNativeWebView.postMessage(JSON.stringify(msg));
  }
  function fail(m) { post({ type: 'error', message: m }); }
  // Diagnostics were used to find the smoothness/recovery issues; kept as no-ops so the call
  // sites still document the pipeline stages without spamming the bridge. Flip to post({...}) to
  // debug again.
  function log(m) { /* no-op */ }

  // transcodes when it must). Sending an unsupported codec yields a black, silent stream.
  // iPhone does NOT expose plain MediaSource. Apple ships ManagedMediaSource instead (iOS 17.1+),
  // a variant that lets the system throttle buffering on cellular. Same API surface for our use,
  // but it must be constructed from its own class and the element must opt out of remote playback
  // BEFORE the source is attached, or attaching throws.
  var MS = window.ManagedMediaSource || window.MediaSource;

  function probeCodecs() {
    var vids = ['avc1.640029','avc1.64002A','avc1.640033','avc1.42E01E','hvc1.1.6.L153.B0'];
    var auds = ['mp4a.40.2','mp4a.40.5','flac','opus'];
    var ok = function (mime) { return !!MS && MS.isTypeSupported(mime); };
    var out = [];
    vids.forEach(function (c) { if (ok('video/mp4; codecs="' + c + '"')) out.push(c); });
    auds.forEach(function (c) { if (ok('audio/mp4; codecs="' + c + '"')) out.push(c); });
    return out.join(',');
  }

  // ManagedMediaSource gates feeding on a metered link: it drops ms.streaming to false to say
  // "stop appending, the buffer is full enough for now" and raises startstreaming when it wants
  // more. On cellular, ignoring this and appending anyway is what tripped the decoder into
  // MEDIA_ERR_DECODE and the endless reconnect loop. Respect the flag; when it is undefined (plain
  // MediaSource, or Wi-Fi where it stays true) this reads as "always feed", i.e. the old behaviour.
  function wantsData() {
    return ms && ms.streaming !== false;
  }

  function flush() {
    if (!sb || sb.updating || queue.length === 0 || !wantsData()) return;
    // Feed the decoder contiguously — never drop an fMP4 segment, which would leave a gap that
    // itself decodes to an error. If a pause let the queue grow, appending it all is fine: the
    // decoder stays happy and seekToLive() skips the playhead forward to the live edge afterwards,
    // so the backlog costs latency, not correctness.
    try { sb.appendBuffer(queue.shift()); } catch (e) { queue = []; }
  }

  // range and the picture stays black. Jump into the range, and keep near the live edge.
  function seekToLive() {
    try {
      if (!sb || !sb.buffered.length) return;
      var end = sb.buffered.end(sb.buffered.length - 1);
      var start = sb.buffered.start(0);
      // Each correction is a visible jump, so the goal is to need them rarely. Landing at end-0.3
      // (what the web does) leaves almost no headroom: playback drifts back over the threshold
      // within seconds and jumps again. Landing further back buys ~1s of slack, which on this
      // device turned a steady stutter into one occasional correction — at the cost of a little
      // more delay, which the measurements show we can afford (drift was under 1s throughout).
      if (v.currentTime < start || v.currentTime > end) v.currentTime = Math.max(start, end - 1.0);
      else if (end - v.currentTime > 2.5) v.currentTime = end - 1.0;
      // Trimming mid-playback can itself hiccup, so do it rarely and keep well clear of the
      // playhead rather than every 12 seconds.
      if (!sb.updating && end - start > 30) { try { sb.remove(start, end - 15); } catch (e) {} }
      if (v.paused) v.play().catch(function () {});
    } catch (e) {}
  }

  if (!MS) { fail('No Media Source API in this WebView (neither ManagedMediaSource nor MediaSource).'); return; }

  var codecs = probeCodecs();
  if (!codecs) { fail('The player supports none of the camera codecs (H.264/HEVC).'); return; }

  post({ type: 'phase', phase: 'connecting' });
  ms = new MS();
  // Required by ManagedMediaSource, and harmless otherwise: without it, attaching the source throws.
  v.disableRemotePlayback = true;
  v.src = URL.createObjectURL(ms);

  // The two halves of the ManagedMediaSource contract. endstreaming needs no handler beyond
  // letting ms.streaming go false (wantsData checks it); startstreaming means resume, so drain
  // whatever piled up while paused.
  ms.addEventListener('startstreaming', function () { flush(); });
  ms.addEventListener('endstreaming', function () { /* ms.streaming is now false; flush() will wait */ });

  var msOpen = false, wsOpen = false, sent = false;
  // The handshake must go out only once BOTH the socket is open AND MediaSource is ready —
  // either order is possible, so send from whichever finishes last.
  function handshake() {
    if (sent || !msOpen || !wsOpen) return;
    sent = true;
    log('handshake: ' + codecs);
    try { ws.send(JSON.stringify({ type: 'mse', value: codecs })); } catch (e) { fail('handshake failed: ' + e); }
  }

  ms.addEventListener('sourceopen', function () { log('sourceopen'); msOpen = true; handshake(); });

  ws = new WebSocket(${JSON.stringify(url)});
  ws.binaryType = 'arraybuffer';

  ws.onopen = function () { log('ws open'); wsOpen = true; handshake(); };
  ws.onerror = function () { fail('Could not reach the live gateway.'); };
  ws.onclose = function (e) { log('ws closed code=' + e.code); post({ type: 'phase', phase: 'stalled' }); };

  ws.onmessage = function (ev) {
    if (typeof ev.data === 'string') {
      var msg = {};
      try { msg = JSON.parse(ev.data); } catch (e) { return; }
      log('server: ' + ev.data.slice(0, 120));
      if (msg.type === 'mse' && ms.readyState === 'open' && !sb) {
        try {
          sb = ms.addSourceBuffer(msg.value);
          log('sourcebuffer ' + msg.value);
          sb.mode = 'segments';
          sb.addEventListener('updateend', function () { flush(); seekToLive(); });
        } catch (e) { fail('Codec rejected by the player.'); }
      }
      return;
    }
    if (!gotBinary) { gotBinary = true; log('first media chunk ' + ev.data.byteLength + 'B'); }
    queue.push(ev.data);
    flush();
  };

  v.addEventListener('playing', function () { post({ type: 'phase', phase: 'live' }); });
  v.addEventListener('waiting', function () { post({ type: 'phase', phase: 'stalled' }); });
  v.addEventListener('error', function () { fail('video element error: ' + (v.error && v.error.code)); });
  // Autoplay can still be refused even muted; surface it instead of sitting on "Connecting".
  v.play && v.play().catch(function (e) { log('autoplay refused: ' + e.name); });
})();
</script>`;
}
