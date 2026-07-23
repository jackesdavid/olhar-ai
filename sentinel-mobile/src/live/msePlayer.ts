// The live player, as a self-contained HTML document hosted in a WebView.
//
// React Native has no `MediaSource`, and the gateway speaks fragmented MP4 over a WebSocket
// (go2rtc's MSE protocol) — not HLS or a plain URL a native player could open. So the phone reuses
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
  | { type: "error"; message: string };

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
  var ws = null, ms = null, sb = null, queue = [];

  function post(msg) {
    if (window.ReactNativeWebView) window.ReactNativeWebView.postMessage(JSON.stringify(msg));
  }
  function fail(m) { post({ type: 'error', message: m }); }

  // Offer only codecs this engine can actually decode; go2rtc picks a matching track (and
  // transcodes when it must). Sending an unsupported codec yields a black, silent stream.
  function probeCodecs() {
    var vids = ['avc1.640029','avc1.64002A','avc1.640033','avc1.42E01E','hvc1.1.6.L153.B0'];
    var auds = ['mp4a.40.2','mp4a.40.5','flac','opus'];
    var ok = function (mime) { return 'MediaSource' in window && MediaSource.isTypeSupported(mime); };
    var out = [];
    vids.forEach(function (c) { if (ok('video/mp4; codecs="' + c + '"')) out.push(c); });
    auds.forEach(function (c) { if (ok('audio/mp4; codecs="' + c + '"')) out.push(c); });
    return out.join(',');
  }

  function flush() {
    if (!sb || sb.updating || queue.length === 0) return;
    try { sb.appendBuffer(queue.shift()); } catch (e) { queue = []; }
  }

  // go2rtc's fMP4 timestamps are not zero-based, so the playhead starts OUTSIDE the buffered
  // range and the picture stays black. Jump into the range, and keep near the live edge.
  function seekToLive() {
    try {
      if (!sb || !sb.buffered.length) return;
      var end = sb.buffered.end(sb.buffered.length - 1);
      var start = sb.buffered.start(0);
      if (v.currentTime < start || end - v.currentTime > 1.5) v.currentTime = Math.max(start, end - 0.3);
    } catch (e) {}
  }

  if (!('MediaSource' in window)) { fail('This device cannot play the camera video codecs.'); return; }

  var codecs = probeCodecs();
  if (!codecs) { fail('No supported video codec.'); return; }

  post({ type: 'phase', phase: 'connecting' });
  ms = new MediaSource();
  v.src = URL.createObjectURL(ms);

  var msOpen = false, wsOpen = false, sent = false;
  // The handshake must go out only once BOTH the socket is open AND MediaSource is ready —
  // either order is possible, so send from whichever finishes last.
  function handshake() {
    if (sent || !msOpen || !wsOpen) return;
    sent = true;
    try { ws.send(JSON.stringify({ type: 'mse', value: codecs })); } catch (e) {}
  }

  ms.addEventListener('sourceopen', function () { msOpen = true; handshake(); });

  ws = new WebSocket(${JSON.stringify(url)});
  ws.binaryType = 'arraybuffer';

  ws.onopen = function () { wsOpen = true; handshake(); };
  ws.onerror = function () { fail('Could not reach the live gateway.'); };
  ws.onclose = function () { post({ type: 'phase', phase: 'stalled' }); };

  ws.onmessage = function (ev) {
    if (typeof ev.data === 'string') {
      var msg = {};
      try { msg = JSON.parse(ev.data); } catch (e) { return; }
      if (msg.type === 'mse' && ms.readyState === 'open' && !sb) {
        try {
          sb = ms.addSourceBuffer(msg.value);
          sb.mode = 'segments';
          sb.addEventListener('updateend', function () { flush(); seekToLive(); });
        } catch (e) { fail('Codec rejected by the player.'); }
      }
      return;
    }
    queue.push(ev.data);
    flush();
  };

  v.addEventListener('playing', function () { post({ type: 'phase', phase: 'live' }); });
  v.addEventListener('waiting', function () { post({ type: 'phase', phase: 'stalled' }); });
})();
</script>`;
}
