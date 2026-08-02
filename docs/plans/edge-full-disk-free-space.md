# `edge:full-disk` must watch free space — Implementation Plan

**Weakness:** [[architecture-weaknesses]] §4 **Board card:** #162 edge:full-disk must watch FREE SPACE
**Goal:** make the disk alarm fire for the condition a user would call "the box is full".

## The bug

`sentinel-edge/src/lib.rs` fired the alert on `StorageStats.pct`, which is **clip bytes ÷ the
configured `max_storage_mb` quota**. Two things follow:

1. **The quota percentage is a number the box actively keeps low.** The same loop calls
   `evict_if_over_limit()` first, which deletes oldest-first until usage is back under **80%** of
   the quota. So `pct >= 90` is a state the system exists to remove — reachable only between two
   ticks of a 60-second loop.
2. **It says nothing about the disk.** A 500 GB volume at 99% full — logs, images, another
   service, anything sharing it — while recordings sit comfortably inside a 20 GB quota reports
   `pct = 12`. The box is out of space and the alarm is silent. Recording then fails for a reason
   no event ever announced.

This is the §4 pattern: *a check whose success condition can be met without the thing being true.*

## The fix

Alarm on the resource the user would look at — free space on the filesystem holding the recordings
directory — and keep the quota as a second, lesser trigger.

- `StorageStats` gains `mount`, `disk_total_bytes`, `disk_free_bytes`, `disk_pct`, resolved with
  `sysinfo::Disks` (already a dependency, used by `metrics.rs`).
- **Longest-matching mount point wins.** `/` is a prefix of every path, so a first-match search
  reports the root volume for a box whose recordings live on a separate `/data` disk — the wrong
  volume, and wrong in the direction that hides a full one.
- The four fields are `Option`, and an unresolvable mount logs a warning. A zero would read as
  "plenty of room", which is the failure this card is about.
- The event payload gains `reason` (`"disk"` | `"quota"`), `quota_pct`, `disk_pct`,
  `disk_free_bytes`, `disk_total_bytes`, `mount`. `pct` becomes **the percentage that fired**, so
  an existing graph gating on `pct >= 90` reacts to whichever limit was hit instead of reading the
  quota and seeing 12.
- `Settings.pct` and the Settings page are untouched: there `pct` still means "clips against your
  quota", which is what that screen is for.

## Threshold

90% used, matching the existing copy. `available_space()` excludes filesystem-reserved blocks, so
this reads a few points above `df`'s Use% on ext4 — it alarms slightly early, which is the right
direction for a warning.

## Out of scope (v1)

- An absolute free-bytes floor (a 4 TB disk at 90% still has 400 GB). Percentage matches the
  appliance framing and the existing UI copy; revisit if a large-disk box proves noisy.
- Acting on the alert (pausing recording, widening eviction to reclaim space). This card makes the
  condition *visible*; what a tenant does about it is a workflow.
- Cloud-side storage. This is the box's local volume only.
