//! (retired) The pod-shared warm `FrameHub` lived here. It was replaced by the per-run,
//! tenant-edge-bound [`crate::ports::EdgeFrameSource`] — keyed implicitly by the run (not a
//! pod-global camera-IP map), so one flow pod can serve many tenants without collisions.
//! Left intentionally empty; not declared in `lib.rs`.
