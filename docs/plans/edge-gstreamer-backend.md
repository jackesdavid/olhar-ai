# Edge media backend — GStreamer alternative to ffmpeg, per-box flag + measured — Implementation Plan

**Board card:** #138 Edge GStreamer media backend (per-box ffmpeg↔gstreamer switch + measurement)

---

## ⭐ PIVOT — 2026-07-23: the driver is now **phone-as-edge**, not performance

Everything below this section was written under the original framing — *"is GStreamer measurably
lighter than ffmpeg? prove it with a per-box A/B flag before committing."* The operator has reset the
driver, and it changes the mandate, the scope, and the core architecture decision. **Read this section
first; the grounding below is still a valid inventory but its conclusions are superseded here.**

**Why we migrate (the new north-star):** to run the **whole edge inside a cheap Android phone** —
motion detection + frame-to-cloud + everything the Linux edge does — to cut edge hardware cost (a used
phone ≪ a mini-PC/SBC, and it has hardware video decode built in). **iOS is out** (no 24/7 background
camera/CPU); **Android only** (foreground service). See the phone-as-edge follow-on card.

**GStreamer is the unlock, and it's mandatory — not "if the numbers win":**
- Android has **no clean subprocess model.** ffmpeg (subprocess) and **go2rtc** (a Go subprocess)
  cannot be orchestrated on Android the way they are on Linux. GStreamer is the **only** media stack
  that runs natively on both Linux and Android (via the NDK), with **hardware decode** on each
  (VAAPI/NVDEC/VideoToolbox on desktop, **MediaCodec** on Android).
- So performance-parity is **enough**. The macOS field finding that GStreamer showed *no* HW-decode
  win (Phase 3, "closed as no-win") **no longer blocks anything** — we migrate for portability, not
  speed. Parity ⇒ ship.

**Three things this pivot changes vs. the plan below:**

1. **go2rtc is now IN scope** (the grounding below says *"go2rtc stays out of scope"* — superseded).
   Everything go2rtc does for us — the live **MSE/WS** feed, **WebRTC**, RTSP restream/serving,
   `/api/frame.jpeg` snapshots — must be **re-implemented in GStreamer** (`webrtcbin`, `rtspsrc`/
   `rtspclientsink`, `mp4mux fragment-duration` for MSE, `jpegenc` for snapshots), because go2rtc
   itself can't run on the phone. This is the **largest new chunk** — go2rtc is a mature product that
   solved many streaming edge cases; reimplementing its surface in GStreamer is real engineering,
   flagged honestly, likely its own phase.

2. **The architecture flips from `gst-launch` subprocess → linked `gstreamer-rs` bindings.** The plan
   below makes "spawn `gst-launch-1.0` subprocesses (not `gstreamer-rs`, to keep the #79 static-musl
   build)" its **load-bearing design choice**. That is exactly wrong for Android, which has no
   subprocess model — GStreamer there is a **linked library**. So the migration must use
   **`gstreamer-rs` linked bindings**, which run on Linux (dynamic link) *and* Android (NDK lib) from
   one codebase. **Accepted cost:** the edge stops being a "links-nothing" static musl binary (#79) —
   it links libgstreamer. That is the price of one portable media path, and it is worth it.

3. **The per-box `ffmpeg↔gstreamer` flag becomes a transition tool, not the endgame.** The endgame is
   **no ffmpeg and no go2rtc at all** — one GStreamer media path, identical on Linux and Android.

**What still holds from below:** the ffmpeg touchpoint inventory (A1–A6, B3) is an accurate map of
*what* must be reimplemented; the pure-Rust capture core (retina RTSP + depacketizer) stays and feeds
GStreamer; A1 (live decode) is still the first and heaviest slice.

**Dependency direction:** this card (#138, now "retire go2rtc + ffmpeg → one linked GStreamer path")
is the **enabler**; the **phone-as-edge (Android)** card depends on it and cannot start until the
media path is a linked, portable GStreamer.

---

## 🗺️ COMPLETE MIGRATION MAP + STAGED PLAN (2026-07-23) — map everything, share what we can, migrate 1-by-1 with a Mac **and** Jetson Nano test gate at every stage

This is the operator's mandate: *map everything we use, organise what can be shared and improved, then
migrate one capability at a time, each stage validated in-loco on the Mac edge box and the Jetson Nano.*

### Why Mac + Jetson as the two gates
- **Mac** (VideoToolbox HW decode) = the desktop/x86-ish path + the operator's daily test box.
- **Jetson Nano** (aarch64 + NVDEC) = the **best available proxy for the Android target**: both are
  ARM64 with a hardware video decoder, both GStreamer-native. Proving each stage on the Jetson
  de-risks Android before we ever touch a phone. A stage isn't "done" until it passes on **both**.

### The full surface — everything we use ffmpeg + go2rtc for

**ffmpeg (7 touchpoints — inventory verified below in "Grounding"):** A1 live MJPEG decode (hot path),
A2 recording mux (H264 ring → mp4), A3 clip duration probe, A4 hwaccel capability probe, A5 snapshot
fallback, A6 presence check, B3 go2rtc-internal ffmpeg for `/api/frame.jpeg`.

**go2rtc (deep — spans edge, app-core, gateway):**
| Responsibility | Where | Consumers |
|---|---|---|
| Sidecar lifecycle (spawn/config/health) | edge `state.rs`, `go2rtc.rs`, `main.rs` | the edge itself |
| RTSP ingest from cameras | go2rtc config (per-camera streams) | everything downstream |
| **`/api/ws` — live MSE over WebSocket** | served by go2rtc | **gateway `proxy.rs` → web player + mobile WebView player** |
| **`/api/frame.jpeg` — snapshot** | served by go2rtc (shells to ffmpeg, B3) | gateway passthrough; app-core `go2rtc.rs` ("Get current frame" node) |
| `/api/streams`, `/stream.html`, player assets | served by go2rtc | gateway passthrough |
| WebRTC | go2rtc `webrtcbin` | (minor today — 1 ref) |
| Tailnet exposure (MagicDNS TLS) | edge `tailscale.rs` | gateway reaches it over the tailnet |

### The load-bearing shared seam — **keep the go2rtc HTTP contract, swap the engine**
The single most important design fact: **`/api/ws` (MSE) + `/api/frame.jpeg` is a contract**, and its
consumers are the **gateway** (`proxy.rs`), the **web player** (`sentinel-app` Live tab), and the
**mobile player** (the WebView MSE player, card #139). If the edge keeps serving that exact HTTP
surface — but backed by **GStreamer** (`mp4mux fragment-duration` fed over a WS for MSE; `jpegenc` for
the frame) instead of go2rtc — then **the gateway, the web player and the mobile player change nothing.**
The whole migration is contained inside the edge. This is what "organise what can be shared" buys us:
one HTTP seam, three unchanged consumers.

### What is shared / improved by the migration
- **Pure-Rust capture core stays and is reused** — retina RTSP + the H264 depacketizer (`recording.rs`)
  already exist; they feed a GStreamer `appsrc` instead of go2rtc. One ingest, many sinks.
- **One decode, many consumers** — today A1 (live decode), B3 (snapshot), and A2 (recording) each spin
  their own media process. GStreamer lets a **single decoded pipeline** fan out to motion/detection +
  snapshot + MSE + recording via `tee`. Fewer processes, less CPU, one warm decoder.
- **Hardware decode finally APPLIED** — today `hwaccel.rs` detects `vaapi|nvdec|videotoolbox|…` and
  **never uses it**. GStreamer's decoder elements (`vtdec` Mac, `nvv4l2decoder`/NVDEC Jetson,
  `amcviddec`/MediaCodec Android) apply it for real — the one genuine *improvement*, not just parity.
- **`gstreamer-rs` linked, one codebase** — the same Rust pipeline builds on Mac, Jetson, and Android
  (NDK). (Supersedes the plan-below "spawn gst-launch subprocess" design — see the Pivot section.)

### Staged migration — 1 capability per stage, each gated on Mac **and** Jetson

> Order = highest-value + lowest-risk first, and each stage leaves the box fully working (ffmpeg/go2rtc
> stay for anything not yet migrated). Every stage: implement → test Mac → test Jetson → measure
> CPU/mem → commit. No stage advances until both boxes pass.

- **Stage 0 — Foundation: linked `gstreamer-rs` in the edge.** Add the dep, build on Mac, cross-compile
  for Jetson (L4T aarch64). Prove a trivial `videotestsrc ! … ! fakesink` pipeline runs from Rust on
  both. **Gate:** the edge binary links libgstreamer and runs a pipeline on Mac + Jetson. (This is the
  #79 static-build tradeoff, made real and measured here.)
- **Stage 1 — A1 live decode (RTSP → frames) with HW decode.** The heaviest touchpoint. `rtspsrc !
  <hwdecode> ! appsink` feeding the pure-Rust motion/detector. HW element per platform (`vtdec` /
  `nvv4l2decoder`). **Gate:** motion + detection run off GStreamer frames on both boxes; CPU measured
  vs ffmpeg baseline (parity is enough).
- **Stage 2 — Snapshot (`/api/frame.jpeg` + "Get current frame" node).** `jpegenc` off the Stage-1 warm
  pipeline, replacing go2rtc's frame.jpeg (kills B3, the ffmpeg-in-go2rtc path). **Gate:** the workflow
  "Get current frame" and the gateway snapshot passthrough return a frame on both boxes.
- **Stage 3 — Recording mux (A2).** H264 ring → fragmented MP4 via `mp4mux`, replacing the ffmpeg
  muxer; A3 duration via GStreamer too. **Gate:** record + play back a clip on both boxes.
- **Stage 4 — The live serving contract (`/api/ws` MSE).** The big go2rtc replacement: the edge serves
  `/api/ws` itself (GStreamer `mp4mux fragment-duration` → WS), **contract-identical** so the gateway +
  web + mobile players are untouched. **Gate:** live view works end-to-end through the gateway, on both
  boxes, in the existing web AND mobile players — no client change.
- **Stage 5 — Retire go2rtc + ffmpeg.** WebRTC path if still needed; then drop both subprocesses, the
  `install.sh` deps, and the presence checks (A4/A6). **Gate:** full parity on both boxes with **neither
  ffmpeg nor go2rtc installed** — the portability proof, and the green light for the phone-as-edge card.

### Open decisions (staged plan)
1. **`/api/ws` MSE re-impl fidelity** — go2rtc's MSE has codec-negotiation edge cases; match its
   `{type:"mse", value:codecs}` handshake exactly (the web + mobile players already speak it) rather
   than invent a protocol.
2. **Jetson cross-build** — cross-compile with the L4T GStreamer, or build on-device? Decide at Stage 0.
3. **Static build (#79) fate** — linking libgstreamer ends the musl-static release. Dynamic-link +
   bundle the GStreamer runtime per platform (Mac/Jetson/Android), or a per-target build. Stage 0 call.

---

## Matrix — funcionalidade × ffmpeg × GStreamer (estado real, não o planejado)

| # | Funcionalidade | ffmpeg | GStreamer | Vencedor / observação |
|---|---|---|---|---|
| A1 | **Decode ao vivo** (frame hub — preview/motion/face, `capture.rs`) | ✅ Único caminho até v0.5.52. Sem hwaccel aplicado (só detectado). | ✅ Shipped (v0.5.53), `decodebin` codec-agnóstico, mesmo wire format (JPEGs concatenados) — `read_mjpeg` não mudou. | **Empate funcional.** Nenhum aplica HW-decode hoje (GStreamer bloqueia de propósito, ver Fase 3). |
| A2 | **Mux de gravação** (ring H264 → mp4, `-c:v copy`) | ✅ Único caminho, sempre. | ❌ **Tentado e rejeitado** — `h264parse` não consegue inventar PTS pra um elementary stream sem timing (precisaria de `appsrc`/bindings, proibido pela #79). | **ffmpeg, permanente.** `media_backend` não tem efeito aqui — `spawn_muxer` ignora o flag. |
| A3 | **Probe de duração do clipe** | ✅ `ffprobe`, único caminho. | ❌ Não implementado (dependia de A2, que foi rejeitado). | **ffmpeg.** |
| A4 | **Probe de capacidade HW-decode** (`hwaccel.rs`) | ✅ Detecta `vaapi/qsv/cuda/videotoolbox/v4l2m2m`, mas **nunca aplica** — só reportado pro cloud (`X-Edge-Hwaccel`). | ⚠️ Substituído por `gstreamer_ok()` — só checa se o binário existe no PATH, **não** checa cobertura de codec (achado real: Jetson sem `-libav` reporta `ok` e não decodifica nada). | **Nenhum é robusto.** ffmpeg detecta mas não usa; gstreamer nem detecta cobertura. |
| A5 | **Snapshot de credencial** (Devices → Test credentials) | ✅ Único caminho, decisão explícita de manter (Fase 2c). | ❌ Fora de escopo — sem valor de medição (uso pontual, não contínuo). | **ffmpeg, por decisão.** |
| A6 | **Presence check do binário** | ✅ `ffmpeg_ok()`, `OnceLock`. | ✅ `gstreamer_ok()` (mirror), mas sem checar codecs (ver A4). | Ambos existem; GStreamer é mais frágil (ver A4). |
| B3 | **`go2rtc` `/api/frame.jpeg`** (usado por `EdgeFrameSource::current` — nó **"Get current frame"** dos workflows) | ✅ go2rtc chama ffmpeg internamente, sempre. | ❌ **Nunca envolvido** — descoberta de campo (2026-07-23): esse caminho **não** consulta o decoder quente do `capture.rs`, então `media_backend=gstreamer` não afeta o consumidor de ffmpeg de maior frequência num tenant típico. | **ffmpeg, sem alternativa hoje.** Bloqueia o north-star de aposentar o ffmpeg até ser corrigido. |
| — | **Decode acelerado por GPU real (uso efetivo)** | ❌ `-hwaccel` nunca é passado no comando real (só a Fase A4 detecta). | ❌ Bloqueado de propósito (`GST_PLUGIN_FEATURE_RANK` zero-rank) — testado com `nvvidconv`+`nvv4l2decoder` na Jetson, **sem ganho de CPU** (~11% vs ~11%, 2 câmeras ~30% vs ~31%), fechado como não-vantagem. | **Nenhum usa GPU de fato**, e por medição real, não vale a pena para GStreamer neste hardware/carga. |
| — | **Cobertura de codec sem branch manual** | ✅ ffmpeg auto-detecta H264/H265 na entrada. | ✅ `decodebin` também é codec-agnóstico — paridade. | Empate. |
| — | **Modelo de processo** (restrição #79: edge não linka nada, build musl estático) | ✅ Subprocess via `proc::command`. | ✅ Subprocess via `gst-launch-1.0` (mesma restrição respeitada — sem bindings `gstreamer-rs`). | Empate por design. |
| — | **Instalação / bundling** | ✅ Pacote de sistema (Linux apt/dnf/rpm-ostree) ou bundled (macOS/Windows) — sempre presente, é o default. | ⚠️ **Opt-in only** via `install.sh --media-backend gstreamer`; exige SSH pra habilitar numa caixa já instalada (achado de campo B) — sem ação in-app. | **ffmpeg** (zero-fricção); GStreamer tem gap de produto real. |
| — | **Toggle + observabilidade por caixa** | ✅ Sempre ativo, sem toggle necessário. | ✅ Shipped: `gstreamer_ok` (migração `0069`) + badge + `<select>` por caixa em Settings → Edge boxes, gated corretamente. | Ambos OK, mas o gate em si depende do gap de instalação acima. |
| — | **Uso de memória (RSS)** | Baseline não medido formalmente (não é o foco da comparação atual). | Software: paridade aproximada com ffmpeg. Com decode HW (`nvvidconv`): **~2,5× mais RSS** (buffers NVMM) — outro motivo pra manter fechado o HW path. | **ffmpeg/GStreamer-SW empatados; GStreamer-HW perde em memória.** |
| — | **Robustez a decoder de HW "surpresa"** | N/A (não usa GStreamer). | 🐛 **Bug encontrado e corrigido:** `decodebin` autoplugava `nvv4l2decoder` sozinho quando `gstreamer1.0-plugins-bad` estava presente, quebrando `videoconvert` (`not-negotiated`) em TODAS as câmeras da caixa — corrigido fixando SW-only via `GST_PLUGIN_FEATURE_RANK`. | GStreamer tinha uma armadilha real de produção; já mitigada. |

**Leitura rápida:** hoje o GStreamer só está pronto pra produção no touchpoint A1 (o mais pesado, o motivo original do card), com paridade de CPU/funcionalidade e zero ganho de HW-decode medido. Os outros touchpoints (A2/A3/A5/B3) continuam 100% ffmpeg — por decisão deliberada (A2/A3/A5) ou por gap ainda não resolvido (B3, o mais importante pro north-star). A adoção em produção também está travada por um gap de produto (instalação opt-in via SSH, achado B).

## Status (Phase 1 + Phase 2a shipped, in-progress)
- **Phase 1 done:** `RecordingConfig.media_backend` (`"ffmpeg"` default / `"gstreamer"`, `recording.rs`)
  — no migration needed, `recording_config` is already a per-box jsonb column. `capture::Backend`
  enum threaded through `capture::ensure`/`supervise` (called from `frames.rs` alongside
  `face_capture_stream`). `state::gstreamer_ok()` added (mirrors `ffmpeg_ok()`) + `X-Edge-Gstreamer`
  heartbeat header (not yet persisted/surfaced on control — that's cheap to add once a box actually
  needs the badge). A local `EDGE_MEDIA_BACKEND` env override was added for single-box testing
  without a control-side toggle UI (not yet built).
- **Phase 2a done — live MJPEG decode (A1) via GStreamer:** `capture::spawn_gst` runs
  `rtspsrc ! decodebin ! videoconvert ! videorate ! video/x-raw,framerate=6/1 ! jpegenc ! fdsink` —
  **codec-agnostic** (no per-H264/H265 branch) and emits the exact same wire format as ffmpeg
  (concatenated JPEGs on stdout), so `read_mjpeg` needed zero changes. **Software-only** decode, per
  the plan's open decision #5 (clean apples-to-apples vs ffmpeg's un-accelerated baseline).
  A stale/wrong `media_backend=gstreamer` on a box without `gst-launch-1.0` silently falls back to
  ffmpeg (`Backend::resolve`) — never leaves a camera with no working decoder.
- **Validated live on the Jetson Orin Nano test box** (`192.168.10.200`, aarch64/Tegra, real RTSP
  camera): the pipeline decoded 88/88 valid JPEG frames at the target 6fps with zero errors; a full
  `sentinel-edge` build with `EDGE_MEDIA_BACKEND=gstreamer` ran the supervised decoder loop for 80+s
  with no watchdog kills/respawns (`capture-decoder[…] backend=gstreamer`, one stable `gst-launch-1.0`
  process, ~17% CPU on a 6-core Tegra — software H264 decode).
- **Load-bearing discovery:** this Jetson's stock `gstreamer1.0-plugins-*` did NOT include a
  software H264 decoder — `decodebin` failed with "missing plugin" until `gstreamer1.0-libav` was
  installed. **`install.sh` (Phase 3) must add `-libav` alongside `-{base,good,bad,ugly}`** or the
  SW-only path silently has no decoder on a fresh box (`gstreamer_ok()` only checks the CLI binary,
  not codec coverage — worth hardening later).
- Open decisions resolved by the above: **#1 subprocess model** (`gst-launch-1.0` via `proc::command`,
  confirmed working) · **#2 flag transport** (extend `RecordingConfig`, confirmed) · **#4 v1 scope**
  (A1 first, confirmed) · **#5 SW-only for 2a** (confirmed, and it's what surfaced the `-libav` gap).
- **Control-side toggle + health shipped:** `gstreamer_ok` is now a real column (migration `0069`,
  mirrors `ffmpeg_ok`) reported on `X-Edge-Gstreamer`, surfaced on Settings → Edge boxes. Once a box
  reports it, a per-box `<select>` (ffmpeg/gstreamer) PATCHes `recording_config.media_backend`
  (spreading the box's existing config so custom pre_roll/storage/etc. survive the write) — no
  tenant-wide toggle, this is per-box on purpose for A/B, and it's gated behind `gstreamer_ok` so it
  can't be flipped on a box that would just silently fall back anyway. Takes effect on next restart
  (`RecordingManager` reads `media_backend` once at boot, like every other `recording_config` field).
- **`install.sh` updated:** `--media-backend gstreamer` / `SENTINEL_MEDIA_BACKEND=gstreamer` installs
  `gstreamer1.0-{tools,plugins-base,plugins-good,plugins-bad,plugins-ugly,libav}` (Linux, mutable +
  rpm-ostree-layered) or the Homebrew equivalents (macOS) — opt-in only, ffmpeg install is untouched
  and still the default. Writes `EDGE_MEDIA_BACKEND=$MEDIA_BACKEND` to `edge.env` either way.
- **Phase 2b (recording mux, A2) tried and REJECTED — real blocker, not a config miss:**
  `fdsrc fd=0 ! h264parse ! mp4mux faststart=true ! filesink` fails with **"Buffer has no PTS"**
  for a raw Annex-B elementary stream fed over stdin. Confirmed on the Jetson test box with a
  *clean* H264 sample (captured via `ffmpeg -c:v copy`, ruling out the go2rtc-repackaged-NAL
  corruption that motivated the edge's own custom `H264Reassembler` in the first place) — tried
  `filesrc` (rules out a pipe-specific issue) and `fdsrc do-timestamp=true`/`is-live` (not a
  property on this fdsrc), with and without an explicit `framerate` cap before/after `h264parse`.
  A synthetic `videotestsrc ! x264enc ! h264parse ! mp4mux` pipeline muxes fine in the same
  environment — so `mp4mux` itself works, the problem is specifically that `h264parse` can't
  invent PTS for a byte-stream with no timing metadata (this camera's SPS has no VUI timing info).
  The standard fix is `appsrc` with manually-assigned buffer PTS — which needs `gstreamer-rs`
  bindings, contradicting decision #1 (subprocess-only, because #79 rules out the edge linking
  anything). **`spawn_muxer` now ignores `media_backend` and always uses ffmpeg for A2/A3** —
  `RecordingConfig.media_backend` only drives `capture.rs`'s A1 live decode for now. Revisit only
  if the project ever revisits the "no bindings" constraint.

  > **UNBLOCKED 2026-07-24 by the pivot.** That constraint has now been revisited: the migration
  > moved from subprocess `gst-launch` to **linked `gstreamer-rs`** (see the Pivot section — Android
  > has no subprocess model, so bindings are mandatory). The named fix — `appsrc` with
  > manually-assigned buffer PTS — is therefore available, and the feed is already frame-aligned
  > (the ring stores individual frames with `is_keyframe`; `live_rx` delivers one complete frame per
  > message), so each buffer gets a well-defined PTS instead of asking `h264parse` to invent one.
  > A2 is back in scope for Stage 3.
  >
  > **Implementation design (mapped 2026-07-24, not yet built).** The pieces are known:
  > - **Real PTS, not an invented fps.** `H264Frame` already carries `at: Instant` (recording.rs:228),
  >   so pre-roll PTS = `frame.at - first.at`; live frames use arrival time against the same origin.
  >   Deriving PTS from a nominal framerate instead would yield clips that play at the wrong speed.
  > - **Pipeline:** `appsrc (caps video/x-h264,stream-format=byte-stream,alignment=au) ! h264parse !
  >   mp4mux faststart=true ! filesink`, pushing ONE buffer per frame with its PTS set.
  > - **Touches shared plumbing:** the pre-roll extraction (recording.rs:436-485) currently flattens
  >   the ring into one `Vec<u8>`, and `live_sink`/`live_rx` is a `Vec<u8>` channel — both must carry
  >   per-frame timestamps for the above. That is why this is NOT a drop-in behind the feature flag
  >   the way A1/B3/A5 were: the ffmpeg path shares that plumbing.
  > - **A3 (duration)** then comes free from the same pipeline (query the muxed duration) or a
  >   `discoverer` call, replacing `ffprobe`.
  >
  > **Why it wasn't shipped in the same session as the unblock:** unlike every other slice migrated so
  > far, a PTS mistake does not fail loudly — it produces a *playable-looking* clip with the wrong
  > duration or speed. It is only verifiable by recording and playing one back on a real box, and it
  > refactors a path with hard-won fixes in it (pre-roll race, IDR gating). It should be built with
  > that verification in hand.
- **Phase 3 hwaccel (GStreamer HW-decode element) — retried with a rigorous A/B, CLOSED as no-win.**
  Found the cleaner integration first: swap `videoconvert` → **`nvvidconv`** (Tegra's own converter,
  from `nvidia-l4t-gstreamer`, already installed) — it bridges NVMM ↔ system memory generically, so
  `decodebin ! nvvidconv ! …` lets `decodebin` freely autoplug `nvv4l2decoder` (HW) *or* `avdec_h264`
  (SW) without the `not-negotiated` break that `videoconvert` hits on NVMM. No `GST_PLUGIN_FEATURE_RANK`
  pin needed, no explicit per-codec depay/parse branch (`decodebin` stays codec-agnostic) — strictly
  simpler than the earlier explicit-pipeline attempt. Confirmed working live (`NvMMLiteOpen`/
  `NvMMLiteBlockCreate` in the log = real HW decode path).
  **Measured properly this time** (`ps %cpu` sampled every 8s over a 45–50s steady-state window, not a
  single eyeballed sample), on the real Jetson test box, two passes:
  - **Single camera** (640×480 sub-stream, 6fps target): SW ~11.0% CPU (steady, 6 samples) vs HW
    ~10.8% CPU (steady, 6 samples) — statistically the same. HW path uses **~2.5× the RSS** (NVMM
    buffer overhead: 1.8% vs 0.7% `%mem`).
  - **Both cameras concurrently** (realistic load — 640×480 + 1280×720, the box's actual 2-camera
    setup): SW steady-state total ~28–31% CPU vs HW steady-state total ~30–32% CPU — again no
    measurable win, HW trending slightly *higher* under combined load.
  **Conclusion: HW decode is not worth it here.** At this fps/resolution the bottleneck is the RTSP
  depay + JPEG re-encode + `videorate` stage, not H264 decode itself — offloading decode to the
  Tegra's fixed-function block doesn't reduce total CPU, it just relocates a cheap-in-CPU-terms step
  and adds memory overhead. **Closed, not "shelved pending measurement" anymore** — Phase 0's own
  gate ("once SW parity is measured") is now satisfied and the answer is negative. Don't re-attempt
  on this class of hardware/workload; the SW-only pin shipped in v0.5.53 (`GST_PLUGIN_FEATURE_RANK`
  zero-ranking every known HW decoder in `capture.rs::spawn_gst`) stays as the permanent design, not
  a placeholder — it's simpler, uses less memory, and performs the same. A future revisit would only
  make sense at meaningfully higher fps/resolution/camera-count where decode itself (not encode)
  becomes the dominant cost.
- **Phase 2c (snapshot fallback, A5) decided: keep ffmpeg.** It's a one-off credential-test
  snapshot (Devices → Test credentials), not a continuous operation — no measurement value, and
  `app_core::onvif::client::snapshot_from_rtsp` is shared library code with only one caller
  (edge `http.rs`), so converting it would add cross-crate backend plumbing for zero payoff.
- **Bug found + fixed: `decodebin` could autoplug a HW decoder it then couldn't feed into
  `videoconvert`, breaking capture for EVERY camera on the box.** Root cause of "boxes with
  media_backend=gstreamer aren't actually decoding": once `gstreamer1.0-plugins-bad` is also
  installed (which `install.sh` now does for a future Phase 3, alongside `-good`/`-libav`),
  `decodebin` on the Jetson test box started preferring `nvv4l2decoder` (from
  `nvidia-l4t-gstreamer`, already installed) over `avdec_h264` (from `-libav`) — `nvv4l2decoder`'s
  output is Tegra `NVMM` memory, which the pipeline's `videoconvert` can't accept, so EVERY camera
  failed with `not-negotiated` and the box silently sat on go2rtc's `frame.jpeg` fallback forever
  (`capture-decoder[…]: repeated gstreamer failures — backing off to 30s`). Reproduced live via
  `gst-launch-1.0` against the box's real go2rtc-reserved RTSP streams (all 4: preview/HD ×
  2 cameras), confirmed the fix by zero-ranking the known HW-decoder element names via
  `GST_PLUGIN_FEATURE_RANK` on the `gst-launch-1.0` child (`capture.rs`'s `spawn_gst`) — verified
  end-to-end against the real edge binary + real service (both cameras streaming steadily via
  gstreamer with motion detection firing), then reverted the test box back to ffmpeg/the shipped
  binary. This pins Phase 2a's "software decode only for now" design intent for real, instead of
  it being an accident of which packages happen to be on a box — protects against the same class
  of surprise from VAAPI/QSV/D3D11/VideoToolbox HW decoders on other platforms too.
- **Not yet done:** Phase 0's *pipeline* half (tag `EdgeMetrics` heartbeat with active backend so
  control can group CPU/mem by backend over time in production) — the hwaccel A/B itself is now
  answered (above), but still via manual `ps` sampling on the test box, not the metrics pipeline.
  Low priority now that HW decode is closed — the only remaining use would be tracking ffmpeg-vs-
  gstreamer SW CPU on real tenant boxes, which isn't blocking anything.

## Field findings — macOS box, 2026-07-23 (first non-Jetson enablement)
Enabling GStreamer on the operator's Mac (`MacInstalledBox`, 0.5.52, macOS x86_64, `videotoolbox`)
surfaced one **scope-changing** defect and four UX gaps. It now runs
`capture-decoder[…] backend=gstreamer` (confirmed in the log, `gstreamer_ok=t`,
`recording_config.media_backend=gstreamer`).

### A. SCOPE CHANGE — `Get current frame` never touches the GStreamer decoder
Phase 2a assumed A1's warm decoder covers the hot frame path and B3 (`go2rtc`'s internal ffmpeg for
`/api/frame.jpeg`) is only a cold-start fallback. **That is false for workflows.** The workflow node
runs in `sentinel-flow` and resolves frames through `EdgeFrameSource` (`crates/app-core/src/ports.rs`):

```rust
impl FrameSource for EdgeFrameSource {
    fn snapshot(&self, device) -> … { go2rtc::frame_from(&agent, &self.base, &device.ip) }
    fn current(&self, device, _max_age_ms) -> … { Ok((self.snapshot(device)?, 0)) } // ignores max_age
}
```

So **every** `Get current frame` — including a `Cron 2s` graph like the seeded "Alerta de pessoa"
template — goes straight to `frame.jpeg`, i.e. **go2rtc shelling out to ffmpeg**, regardless of
`media_backend`. It never consults `capture.rs`'s warm decoder, and it ignores `max_age_ms` (always
a fresh snapshot at age 0). The warm grabber described in the node's docs exists only *inside* the
edge (`frames.rs`, for motion/tracking/face), not on this remote path.

**Consequence:** the continuous, highest-frequency ffmpeg consumer in a typical tenant is untouched
by the GStreamer work, and the north-star (retire ffmpeg) cannot be reached without addressing it.
**Likely cheap fix:** expose an edge endpoint serving `capture::latest_fresh` and have
`EdgeFrameSource::current` use it honouring `max_age_ms`, falling back to `frame.jpeg` only when no
fresh frame exists — exactly what `frames.rs` already does internally. This makes B3 a real
cold-start fallback instead of the default, which is what Phase 2a assumed all along.

### B. Enabling GStreamer requires SSH — no in-app path (chicken-and-egg)
- The control toggle is **gated on `gstreamer_ok`**; `gstreamer_ok` is true only if `gst-launch-1.0`
  is on the box PATH; the binary only arrives via `install.sh` with the **opt-in**
  `--media-backend gstreamer`. There is **no in-app action** to provision it.
- This contradicts the project rule *never require SSH/CLI/restart for a product capability — it is
  an in-app action or automatic*. Choosing a media backend is a product capability.
- Consider installing GStreamer by default (the north-star retires ffmpeg anyway), or an in-app
  "prepare this box" action.

### C. `--update` silently ignores `--media-backend`
`install.sh` parses `--media-backend` into `MEDIA_BACKEND`, but update mode **`exit 0`s** (the
`if [ "$UPDATE" = "1" ]` block) *before* the `1c. runtime dep: gstreamer` block. So
`--update --media-backend gstreamer` accepts the flag and installs nothing, with no warning. At
minimum warn; better, honour it.

### D. The edge never logs the GStreamer probe
Boot logs `ffmpeg found on PATH` but there is **no equivalent line for GStreamer** — `gstreamer_ok()`
just returns false silently. An operator has no way to tell "not installed" from "installed but not
found". Log the probe result + resolved path for both backends.

### E. The UI does not explain the gate
When `gstreamer_ok` is false the per-box select is simply absent, with no explanation. Show
"GStreamer not detected on this box" plus what to do — otherwise it reads as a broken/missing
feature (it did).

**Also worth noting:** `gstreamer_ok()` only probes the CLI binary, not codec coverage — already
flagged as "worth hardening later" and independently confirmed by the Jetson's missing `-libav`.
A box can report `true` and still have no working H264 decoder.

**Goal:** Let each edge box run **GStreamer instead of ffmpeg** for every media operation the edge
does, switchable per box via a flag pushed from control, so we can A/B **measure CPU/mem** and decide
whether GStreamer is lighter. ffmpeg stays the default and both coexist behind the flag **for now**.

**North star:** the per-box flag is a **migration path, not a permanent fork**. If the numbers hold
up (GStreamer measurably lighter, at parity on every touchpoint below), the endgame is to **flip the
default to GStreamer and then retire ffmpeg entirely** — drop the ffmpeg subprocesses, the
`install.sh` ffmpeg dependency, and (pending the go2rtc `frame.jpeg` decision, B3 below) the ffmpeg
system/bundled binary. The coexistence phase exists only to prove the numbers before that removal.

## Grounding — every ffmpeg touchpoint on the edge (the seam surface)
All edge ffmpeg use goes through one spawn helper — `crate::proc::command(...)`
(`sentinel-edge/src/proc.rs:9`) — except the app-core snapshot fallback (raw `Command`). Seven
touchpoints (verified inventory):

| # | Operation | Location | Mechanism | HW-accel today |
|---|---|---|---|---|
| A1 | **Live MJPEG decode** (hot per-camera frame path) | `capture.rs:68-93` (`spawn_ffmpeg`) | `proc::command("ffmpeg")` → MJPEG on `pipe:1` | **none** |
| A2 | **Recording mux** (H264 ring → mp4, `-c:v copy`) | `recording.rs:335-347` | `proc::command("ffmpeg")` stdin pipe | none (stream-copy) |
| A3 | Clip duration probe | `recording.rs:849-858` (`probe_duration`) | `proc::command("ffprobe")` | n/a |
| A4 | HW-decode capability probe | `hwaccel.rs:59-116` | `proc::command("ffmpeg")` ×2 | detects, never applies |
| A5 | Single-frame snapshot fallback | `crates/app-core/src/onvif/client.rs:253` | raw `Command::new("ffmpeg")` | none |
| A6 | ffmpeg presence check | `state.rs:414` (`ffmpeg_ok`) | `ffmpeg -version`, `OnceLock` | n/a |
| B3 | go2rtc-internal ffmpeg for `/api/frame.jpeg` | go2rtc runtime; consumers `app-core go2rtc.rs:112-142`, `frames.rs:185-249` | go2rtc spawns ffmpeg itself | none |

**Facts that shape the design:**
- **A1 is the heavy one** — a long-lived ffmpeg per camera doing CPU MJPEG encode with **no
  hwaccel**. This is what "ffmpeg feels heavy" almost certainly is. It is the prime measurement
  target and the first GStreamer slice.
- **hwaccel is detected but never applied.** `hwaccel.rs` returns `vaapi|qsv|cuda|videotoolbox|
  v4l2m2m|none` but that method is **only reported to the cloud** (health gating,
  `state.rs:459-466` → `X-Edge-Hwaccel`) — no ffmpeg command passes `-hwaccel`. So a GStreamer path
  using HW-decode elements (`vaapih264dec`/`nvh264dec`/`vtdec`) could be a **real win**, not just
  parity.
- **Recording capture is already pure-Rust** (retina RTSP + own H264 depacketizer, `recording.rs`
  §F); ffmpeg is ONLY the mp4 muxer (A2) + duration probe (A3).
- **go2rtc streaming is native** (RTSP/WebRTC/MSE, no ffmpeg). Only `/api/frame.jpeg` uses go2rtc's
  internal ffmpeg (B3) — and A1's warm decoder already bypasses it for the hot path; B3 is only the
  cold-start snapshot fallback. So **go2rtc stays out of scope**.
- **The edge links nothing** (#79, musl static release build) → the GStreamer path must be
  **spawned `gst-launch-1.0` subprocesses** (mirroring the ffmpeg-subprocess model via
  `proc::command`), **not** `gstreamer-rs` bindings (which would link libgstreamer and break the
  static build). This is the load-bearing design choice.
- **Config-pull seam exists:** the edge pulls `GET /config/recording` → `RecordingConfig`
  (`recording.rs:874`, struct `recording.rs:103`) and `GET /config/cameras` (`state.rs:622`), both
  bearer-auth by `BOX_TOKEN`. Per-box column precedent: `tenant_boxes.hwaccel` (migration `0064`).
- **Measurement already exists:** `EdgeMetrics` (#62, `sentinel-edge/src/metrics.rs`,
  `app_core::edge_metrics`) samples CPU/mem/disk/GPU and ships it on the heartbeat
  (`X-Edge-Metrics`). `frames.rs:219-249` already splits go2rtc-grab vs edge-decode cost. Reuse for
  the A/B.
- **Bundling / first test box:** ffmpeg is a Linux system package (`install.sh:195-230`,
  apt/dnf/rpm-ostree) and bundled on macOS/Windows. **The first test + measurement box is the
  operator's macOS dev box** (see the `mac-edge-test-box` setup: manual `/tmp/sentinel-edge`
  install) — GStreamer there is just `brew install gstreamer gst-plugins-{base,good,bad,ugly}
  gst-libav`, so `gst-launch-1.0` is on PATH and we can measure immediately with **zero installer
  bundling**. macOS hwaccel is **videotoolbox → the `vtdec` GStreamer element**, so the HW-decode
  angle is testable on day one. Production bundling (Linux system `gstreamer1-plugins-*`; macOS pkg;
  Windows installer) comes later, once the numbers justify shipping it.

**Test setup (Phases 0–2 run here first):** the operator's **macOS** edge box (`mac-edge-test-box`):
`brew install gstreamer gst-plugins-{base,good,bad,ugly} gst-libav`, then flip that one box's
`media_backend` to `gstreamer` and compare `EdgeMetrics` against its ffmpeg baseline on the same
cameras. macOS decode HW = `videotoolbox`/`vtdec`. Linux appliance validation + install packaging
follow once the Mac numbers look right.

## Phase 0 — Measure first (baseline, no backend change)
Quantify "ffmpeg is heavy" and set up the A/B before writing any GStreamer.
- Tag the `EdgeMetrics` heartbeat with the **active `media_backend`** (default `ffmpeg`) so control
  can group CPU/mem by backend. Normalise by camera count (already known from `/config/cameras`).
- Control-side: a simple per-box CPU/mem-over-time comparison (reuse the box health/metrics already
  stored). Leverage the existing `frames.rs` grab-vs-decode split.

## Phase 1 — The flag + the `MediaBackend` seam (no behaviour change)
- **Migration** (next number — `ls crates/app-core/migrations/`): `ALTER TABLE tenant_boxes ADD
  COLUMN IF NOT EXISTS media_backend text` (mirror `0064`). NULL → edge treats as `ffmpeg`.
- **Control:** serve it to the edge by adding `media_backend` to the `RecordingConfig` response
  (`/config/recording`, `recording.rs:103`) — least new plumbing (already fetched, drives the
  recording+capture managers). Operator sets it via a per-box settings API/toggle (box registry).
- **Edge:** a `MediaBackend` enum (`Ffmpeg` | `Gstreamer`) read from the pulled config, threaded
  into `capture::ensure` (A1), `RecordingManager` (A2/A3), snapshot (A5), and the probes (A4/A6).
  Refactor today's ffmpeg calls behind `MediaBackend::Ffmpeg` — **byte-identical behaviour**. Add
  `gstreamer_ok()` (parallel to `ffmpeg_ok`, `state.rs:414`) reported like `X-Edge-Ffmpeg`.
- Result: ffmpeg still the only working impl and the default; the switch is inert until Phase 2.

## Phase 2 — GStreamer backend, operation by operation (measure each slice)
Each is a buildable slice: implement the `Gstreamer` branch as a `gst-launch-1.0` pipeline, flip a
test box to `gstreamer`, compare `EdgeMetrics` vs an ffmpeg box on the same cameras.
- **2a — Live MJPEG decode (A1), FIRST (heaviest, the whole point):**
  `rtspsrc location=rtsp://127.0.0.1:8554/cam_<ip> ! rtph264depay ! h264parse ! <decoder> !
  videorate ! video/x-raw,framerate=6/1 ! jpegenc quality=… ! fdsink` where `<decoder>` is
  `avdec_h264` (SW) or the HW element for the box's detected hwaccel. Replaces `spawn_ffmpeg`; the
  supervisor/watchdog (`capture.rs:95-167`) stays.
- **2b — Recording mux (A2/A3):** `fdsrc ! h264parse ! mp4mux faststart=true ! filesink` (or
  `splitmuxsink`) fed the same ring bytes; duration via `gst-discoverer-1.0` (replaces ffprobe).
- **2c — Snapshot fallback (A5):** a one-frame `rtspsrc ! … ! jpegenc ! fdsink` (behind the same
  flag in app-core, or keep ffmpeg for the rare fallback — open decision).

## Phase 3 — hwaccel + install/bundling
- Map the box's detected hwaccel (`hwaccel.rs`) → GStreamer HW-decode element for 2a
  (`vaapih264dec` / `nvh264dec` / `vtdec`) — the potential win ffmpeg doesn't currently take.
- `install.sh`: install `gstreamer1-plugins-{base,good,bad,ugly}` + `-libav` (Linux) when the box
  runs gstreamer; `gstreamer_ok()` gates the switch. macOS/Windows bundling deferred.

## Open decisions (grill before building)
1. **`gst-launch-1.0` subprocess vs `gstreamer-rs` bindings** → recommend **subprocess** (preserves
   #79 "edge links nothing" + musl static release; reuses `proc::command`). Bindings only if a
   pipeline needs tight Rust control (appsrc for the recording ring).
2. **Flag transport:** extend `RecordingConfig`/`/config/recording` (recommended, least plumbing) vs
   a dedicated per-box `/config/box` endpoint.
3. **go2rtc `frame.jpeg` (B3):** leave on ffmpeg (recommended — go2rtc is a Go binary; 2a's warm
   decoder already covers the hot path) vs eliminate the fallback entirely.
4. **v1 operation scope:** A1 first (heaviest), then A2, then A5; A4/A6 become `gstreamer_ok` + a GST
   hwaccel probe. Confirm this order.
5. **Apply hwaccel opportunistically in 2a** (a win ffmpeg doesn't get today) or keep 2a SW-only for
   a clean apples-to-apples first measurement?

## Phase 4 (future, gated on the numbers) — retire ffmpeg
Only once GStreamer has proven measurably lighter and at parity across A1–A6 on real boxes:
flip the default `media_backend` to `gstreamer`, roll it out, then **remove ffmpeg** — the edge
subprocesses (A1/A2/A3/A5), the `ffmpeg_ok`/hwaccel-probe ffmpeg calls (A4/A6), the `install.sh`
ffmpeg dependency, and the macOS/Windows bundled ffmpeg binary. The go2rtc `frame.jpeg` residual
(B3) must be resolved first (eliminate the fallback, or accept go2rtc keeping its own ffmpeg).

## Out of scope (v1 — the coexistence phase, not the north star)
- **Removing ffmpeg** — that's Phase 4, gated on the measurements. v1 keeps both, ffmpeg default.
- go2rtc internals / native streaming (unchanged); its `frame.jpeg` ffmpeg residual (B3).
- macOS/Windows GStreamer bundling (Linux appliance first).
