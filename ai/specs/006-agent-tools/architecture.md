# Architecture Notes — Bishop's Tools

## Components

- `src/agent/tools.rs` — `ToolRegistry` (specs array), `ToolSpec { name, input_schema, run }`,
  `ToolContext { store, ui_tx, notif, alerts }`, `ToolUiEvent`. ~14 tools.
- `src/agent/vision.rs` — vision API call describing a snapshot.
- `src/agent/handler.rs` — executes tool calls returned by the model, splices results back.
- Acts on: `src/scan` device store, `src/alerts` store, `src/notification` targets,
  `src/stream` (live feed), `src/device/actions.rs` (snapshot).

## APIs

- Claude tool-use (registry advertised in the request); Claude vision for `describe_camera`.

## Data Flow

Model returns a tool_use block → `agent/handler.rs` looks up the spec by name → `run(input,
&ToolContext)` executes against shared `Arc<Mutex<…>>` stores → string result is returned to
the model as a tool_result. UI-affecting tools emit a `ToolUiEvent` over `ui_tx` consumed by
the app's tool-UI channel.

## Important Decisions

- A single typed registry with a `run` fn per tool keeps tools uniform and discoverable.
- UI effects are decoupled via a channel so tools stay pure w.r.t. rendering.

## Risks

- Tools run on the agent thread; any blocking IO there is acceptable (off the UI thread).
