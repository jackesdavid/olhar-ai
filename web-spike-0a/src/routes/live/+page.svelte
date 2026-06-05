<script lang="ts">
  // go2rtc serves a ready-made WebRTC player at /webrtc.html?src=<stream>. The Rust backend
  // starts go2rtc on launch (see src-tauri/src/lib.rs::spawn_go2rtc). We just embed its player.
  const stream = "camera";
  const playerUrl = `http://localhost:1984/webrtc.html?src=${stream}`;
</script>

<main>
  <header>
    <div>
      <h1>Live camera</h1>
      <p class="sub">
        Phase-0b spike — a <strong>go2rtc</strong> sidecar pulls RTSP and serves
        <strong>WebRTC</strong> (H.264 passthrough, sub-second); the Tauri webview plays it.
        This is the cross-platform-media go/no-go.
      </p>
    </div>
    <a class="back" href="/">← Alerts</a>
  </header>

  <div class="frame">
    <!-- The player runs same-origin to localhost:1984 inside this iframe. -->
    <iframe title="Live camera" src={playerUrl} allow="autoplay; fullscreen"></iframe>
  </div>

  <details class="help">
    <summary>Not seeing video? Run the smoke test on each OS — this is the real test</summary>
    <ol>
      <li>Edit <code>src-tauri/go2rtc.yaml</code> → set <code>camera:</code> to your RTSP URL.</li>
      <li>Put the binary at <code>src-tauri/binaries/go2rtc</code> (see <code>binaries/DOWNLOAD.md</code>).</li>
      <li>Confirm go2rtc is up: open <code>http://localhost:1984</code> in a browser → your stream should play there.</li>
      <li>Then run <code>npm run tauri dev</code> and check this page <em>inside the Tauri window</em>.</li>
    </ol>
    <p class="warn">
      Expected differences by OS webview (the whole point of 0b):
      <br />• <strong>macOS</strong> (WKWebView) — should work.
      <br />• <strong>Windows</strong> (WebView2) — works, but a clean box may lack proprietary
      H.264; go2rtc passthrough of H.264 can hit this. Verify on a fresh machine.
      <br />• <strong>Linux</strong> (WebKitGTK) — WebRTC is often <strong>off by default</strong>;
      this is the most likely failure and the key thing to prove.
    </p>
  </details>
</main>

<style>
  main {
    max-width: 900px;
    margin: 0 auto;
    padding: 32px 20px 56px;
  }
  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 18px;
  }
  h1 {
    margin: 0 0 6px;
    font-size: 22px;
    font-weight: 650;
  }
  .sub {
    margin: 0;
    max-width: 64ch;
    font-size: 13px;
    line-height: 1.5;
    color: #97a0b3;
  }
  .back {
    flex: none;
    padding: 8px 14px;
    border-radius: 8px;
    border: 1px solid #2a3242;
    background: #161b26;
    color: #e6e9ef;
    font-size: 13px;
    text-decoration: none;
  }
  .frame {
    aspect-ratio: 16 / 9;
    width: 100%;
    border: 1px solid #222a38;
    border-radius: 12px;
    overflow: hidden;
    background: #000;
  }
  iframe {
    width: 100%;
    height: 100%;
    border: 0;
  }
  .help {
    margin-top: 18px;
    font-size: 13px;
    color: #97a0b3;
  }
  summary {
    cursor: pointer;
    color: #cdd3df;
  }
  code {
    background: #1b2130;
    border-radius: 5px;
    padding: 1px 5px;
    font-size: 0.85em;
  }
  .warn {
    border-left: 3px solid #d97706;
    padding-left: 12px;
    line-height: 1.7;
  }
</style>
