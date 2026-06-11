---

kanban-plugin: board

---

## Bugs

- [ ] Splash should retry on transient `boot_status` DB errors instead of hanging
- [ ] We should encrypt the passwords in the db column level too


## Backlog

- [ ] Document camera onboarding flow (ONVIF discovery → go2rtc feed)
- [ ] Pricing simulator: surface managed vs self-hosted AI in Settings UI
- [ ] Recipients management UI (add/edit/remove notification targets)
- [ ] Define site/zones model in workflows domain


## ToDo



## In Progress

- [ ] School Student Frequency Case
	  - Plan: [[school-student-frequency]]
	  - Use case: [[school-frequence]]



## Review



## Done

**Complete**
- [x] Devices → Postgres + pgvector migration follow-ups
- [x] Show image (debug) node: renders the input frame inside the node + a button to enlarge in a modal
- [x] Vision detector: local person detection via tract + YOLOX
- [x] Snapshots grabbed from shared go2rtc feed (fast path)
- [x] RunPod `/status` polling on cold-start (IN_QUEUE/IN_PROGRESS)
- [x] CPU-only torch in the AI Docker image




%% kanban:settings
```
{"kanban-plugin":"board","list-collapse":[true,true,false,false,false,false],"show-checkboxes":true,"date-display-format":"YYYY-MM-DD","hide-date-in-title":true}
```
%%