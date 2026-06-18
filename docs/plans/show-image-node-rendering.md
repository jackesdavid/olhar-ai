# Show Image Node: Image Not Rendering — Implementation Plan

**Board card:** Show Image node: image not rendering (inline + fullscreen) (In Progress)
**Goal:** Make the `ShowImage` (debug) node actually display its frame again — both the in-node thumbnail and the fullscreen modal — in the cloud web app.

## Root cause

The `ShowImage` debug node mirrors its input frame to the UI. In the Tauri era this was `emit("workflow-image", { data: <base64 jpeg> })`. The cloud migration deliberately **strips image bytes off the SSE stream** (one UTF-8 stream; a ~300 KB base64 JPEG would head-of-line-block every other event — `sentinel-flow/src/events.rs:6-9`), and `publish_tool_event` emits only a **reference**: `{ tenant, graph, node, label }` (`events.rs:51-53`). The planned out-of-band binary fetch (`GET /runs/{id}/frames/{n}`, named in that comment) was **never built**.

But the frontend still expects the bytes inline — `workflow-image` handler reads a `data` field and sets `src = "data:image/jpeg;base64,${frame}"` (`sentinel-app/src/routes/workflows/+page.svelte:779-787`). `frame` is always `undefined` → broken `src`, so the in-node SVG `<image>` (`:1702`) and the modal `<img>` (`:2350`) both show nothing.

The frame bytes **are** available at emit time: `ToolUiEvent::ShowImage { …, jpeg: Vec<u8> }` (`crates/app-core/src/agent/tools.rs:43`), sent from `engine.rs:373`. They're just dropped before the browser. The fix is to serve them out-of-band, honouring the no-bytes-on-SSE rule (now more important since `workflow-activity` shares that stream).

## Phase 1 — Flow: cache the latest frame + serve it (`sentinel-flow`)
- `state.rs`: add `node_frames: Arc<Mutex<HashMap<(Uuid, u32, u32), Vec<u8>>>>` to `AppState` — latest jpeg per `(tenant, graph, node)`. Tiny + bounded (one entry per debug node, overwritten).
- `engine.rs build_run_context`: in the UI drain thread, before `publish_tool_event`, special-case `ToolUiEvent::ShowImage { graph, node, jpeg, .. }` → `node_frames.insert((tenant, graph, node), jpeg.clone())`. The reference event still publishes as today (bytes dropped there).
- `http.rs`: add `GET /runs/frame?tenant=&graph=&node=` → return the cached bytes as `image/jpeg` (200) or `404` if absent.

## Phase 2 — Control: relay the frame (`sentinel-control`)
- `domain/workflows.rs`: add `GET /workflows/runs/frame?graph=&node=` (auth `workflows.read`, tenant from JWT, `?token=` in query like `events_stream` so an `<img src>` can authenticate). Proxy to flow `/runs/frame?tenant=…&graph=…&node=…`, stream the bytes back with `Content-Type: image/jpeg`.

## Phase 3 — Frontend: fetch out-of-band (`sentinel-app`)
- `lib/data.ts`: add `runFrameUrl(graph, node)` → `${API_BASE}/workflows/runs/frame?graph=…&node=…&token=…` (token from `auth`).
- `+page.svelte` `workflow-image` handler: drop the `data`/base64 path; set `nodeImages[node] = { src: data.runFrameUrl(gid, node) + "&t=" + Date.now(), label }`. The `&t=` cache-busts so each event reloads the `<img>`/`<image>` with the new frame. No other render changes — `:1702` and `:2350` already consume `pv.src` / `imageModal.src`.

## Open decisions
1. **Frame lifetime:** keep the last frame per node indefinitely (survives until pod restart / graph reload), or evict on run start? Recommend keep — the card explicitly wants the preview to persist in the node after a run (`:168`).
2. **Multi-pod:** the cache is per-pod (same single-pod assumption as [[multi-pod-flow-event-fabric]]). Fine today; that card covers the general fix.

## Out of scope (v1)
- Persisting frames to disk/DB (they're transient debug artifacts).
- A history of frames per node (only the latest is shown).
