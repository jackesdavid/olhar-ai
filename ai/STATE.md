# State

Single source of truth for "where are we right now." Keep this file small (~50 lines max). Older history belongs in git or in `ai/specs/<spec-id>/notes.md`.

---

## Header

```
current_spec:    013-workflows-automation
current_role:    reviewer
current_phase:   done
mode:            hitl
started_at:      2026-06-04
```

---

## Recent decisions (rolling — keep last 5 only)

Format: `YYYY-MM-DD | role | decision`

- 2026-06-04 | developer | Implemented 013 Phases 1–6 (src/workflows/ + app wiring). 73 tests green. Deviations accepted (typed PortValue; nodes adapt domain fns).
- 2026-06-04 | developer | Tester send-back fixes: render_link port-picker panel; zoom-adaptive grid; ■/● port markers + cyan port-accurate edges. (Also: CRT window 1200×780→1600×1000, outside 013.)
- 2026-06-04 | tester | Test A PASS (Run-now → alert, live end-to-end) + Test C PASS (bad link rejected) + canvas verified. Tester gate PASS.
- 2026-06-04 | tester | AC verdicts: AC1/2/5/6 PASS (31 unit tests + live); AC3 run-now PASS; AC4 logic PASS.
- 2026-06-04 | reviewer | Operator accepted 013 as complete (formal Reviewer pass waived). **013 DONE.** Known v1 debt documented in tasks.md (60s cron, Run-now bypasses concurrency, transient viewport).

---

## Open send-backs

Format: `from <role> → to <role> | reason | spec`

- _none_

---

## Notes

- This file is read at the start of every `/sdd-orchestrate` invocation.
- The orchestrator updates it after every phase transition.
- When the rolling log fills, drop the oldest entry — don't grow this file.
- If you find yourself wanting more history here, you actually want `ai/specs/<spec-id>/notes.md` or git log.
