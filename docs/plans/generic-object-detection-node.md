# Generic Object Detection Node — Implementation Plan

**Board card:** Generic Object Detection node (ToDo)
**Goal:** Replace the hard-wired COCO class 0 filter in PersonDetection with a configurable class selector so any of the 80 COCO classes can be detected — via a property-panel dropdown or a wired `class` Text input that overrides it.

---

## Context

`PersonDetection` today calls `detector::detect_persons_detailed()`, which runs YOLOX-Nano on all 80 COCO classes but then filters to `PERSON_CLASS = 0`. The underlying `detect()` function already returns every class — the new node just parameterises that filter.

**New node:** `ObjectDetection` — distinct from `PersonDetection` (which stays unchanged for back-compat).

---

## Phase 1 — Backend: parameterise the detector

**`crates/app-core/src/agent/detector.rs`**

1. Add the 80 COCO class name table:
   ```rust
   pub const COCO_CLASSES: [&str; 80] = [
       "person", "bicycle", "car", "motorcycle", "airplane", "bus", "train", "truck", "boat",
       "traffic light", "fire hydrant", "stop sign", "parking meter", "bench", "bird", "cat",
       "dog", "horse", "sheep", "cow", "elephant", "bear", "zebra", "giraffe", "backpack",
       "umbrella", "handbag", "tie", "suitcase", "frisbee", "skis", "snowboard", "sports ball",
       "kite", "baseball bat", "baseball glove", "skateboard", "surfboard", "tennis racket",
       "bottle", "wine glass", "cup", "fork", "knife", "spoon", "bowl", "banana", "apple",
       "sandwich", "orange", "broccoli", "carrot", "hot dog", "pizza", "donut", "cake",
       "chair", "couch", "potted plant", "bed", "dining table", "toilet", "tv", "laptop",
       "mouse", "remote", "keyboard", "cell phone", "microwave", "oven", "toaster", "sink",
       "refrigerator", "book", "clock", "vase", "scissors", "teddy bear", "hair drier",
       "toothbrush",
   ];

   pub fn class_id_for_name(name: &str) -> Option<u32> {
       COCO_CLASSES.iter().position(|&c| c.eq_ignore_ascii_case(name)).map(|i| i as u32)
   }
   ```

2. Add `detect_class_detailed(image_bytes: &[u8], class_id: u32) -> Result<Vec<PersonDetection>, String>` — same as `detect_persons_detailed` but with the class_id parameter instead of the `PERSON_CLASS` constant.

3. Expose it via `VisionPort` trait (in `crates/app-core/src/ports.rs`) alongside the existing `detect_persons_detailed`.

---

## Phase 2 — Backend: new NodeKind + execution

**`crates/app-core/src/workflows/domain.rs`**

1. Add `ObjectDetection` variant after `PersonDetection` (line ~153), with doc comment.

2. Add port constants:
   ```
   OBJECT_DETECTION_IN:  [image (Image, required), class (Text, optional)]
   OBJECT_DETECTION_OUT: [count (Num), confidence (Num), detections (List/PersonArray), ok (Bool)]
   ```
   The `detections` output reuses `PortValue::Person` elements (same bbox+confidence struct) — no new type needed.

3. Wire ports into `NodeKind::ports()` match arm.

4. Add default config — `json!({ "class": "person" })` — in `NodeKind::default_config()`.

5. Add `NodeKind::ObjectDetection` to `NodeKind::ALL`.

**`crates/app-core/src/workflows/node.rs`**

Add `run_object_detection(inputs, config, sys)`:
- Read `class` from wired input (`inputs.get("class")`) first; fall back to `config.get("class")`.
- Resolve class name → class_id via `class_id_for_name()`. Unknown class → return error.
- Call `sys.vision.detect_class_detailed(bytes, class_id)?`.
- Package outputs identically to `run_person_detection` (`count`, `confidence`, `detections` list, `ok`).

Wire it into the `run_node` dispatch match.

---

## Phase 3 — Frontend: property panel + node UI

**`sentinel-app/src/`** (workflow canvas + inspector panel)

1. **Property panel**: when the selected node is `ObjectDetection`, render a `<Select>` dropdown bound to `config.class` populated with all 80 COCO class names (import the list as a TS const or derive from the server's NodeKind descriptor if that surface exists).

2. **Class pin visibility**: the `class` input pin is always shown on the node body (wiring it overrides the dropdown). If wired, grey-out / hide the dropdown in the panel to avoid confusion.

3. **Node label**: display "Detect Objects" as the human label (not "ObjectDetection"). Subtitle could show the configured class name.

4. **`docs/workflow-nodes.md`**: add the catalog entry:
   ```
   **Detect objects** | `image*`, `class` | `count`, `confidence` %, `detections` (**PersonDetection[]**), `ok` | `class` (COCO name, default "person") | Detects any COCO class locally. Wire `class` to override the dropdown; iterate `detections` like PersonDetection. | `Get current frame → Detect objects → (detections) → For each`
   ```

---

## Open decisions

1. **Pin name — `detections` vs `persons`**: plan uses `detections` (generic). Should the output be renamed for clarity, or kept as `persons` for drop-in compat with downstream `BreakPersonDetection`? (Current plan: `detections` — it IS a `PersonDetection[]` element-wise, so `BreakPersonDetection` still works.)

2. **Multi-class mode**: should the node support detecting *multiple* classes in one pass (a `classes` list) and tag each detection with its class? Deferred to v2 — single class keeps the output shape identical to PersonDetection.

3. **`RecordPresence` integration**: the `category` field in RecordPresence is free text today. Should `ObjectDetection` wire its configured class directly into `RecordPresence.category`? Nice-to-have, out of scope here.

---

## Out of scope (v1)

- Multi-class mode (list of classes, tagged detections)
- Confidence threshold as a wired / config input (hardcoded 0.30 stays)
- Deprecating / replacing PersonDetection (keep it; it's used in existing graphs)
- Any UI for adding/removing class options (the 80-class COCO list is fixed)
