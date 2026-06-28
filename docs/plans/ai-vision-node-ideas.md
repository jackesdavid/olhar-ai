# AI/Vision Node Ideas — Research List

**Goal:** Reference list of AI-powered workflow nodes for camera monitoring automation, researched from Roboflow Workflows, AWS Rekognition, smart-building platforms, and retail/industrial analytics systems.

Already shipped or planned (excluded): Person Detection, Object Detection, Face Recognition, Describe Image, Motion Detection, AI If, Detect Objects (COCO), Draw BBox.

---

## Group 1 — Branching / Routing

| Node | What it does | Camera use case |
|---|---|---|
| **AI Extract** | Asks the VLM an open-ended question; returns a string or number (not yes/no) | "What's the license plate?" → feed a watchlist lookup |
| **AI Classify** | Routes exec to one of N named output pins based on a category the VLM assigns | Classify lobby as `empty / single / busy / overcrowded` → different alert branches |
| **Count If** | Routes exec when a detection count meets a numeric threshold (>, <, =) | Open a second checkout when queue > 6 people |

## Group 2 — Extraction

| Node | What it does | Camera use case |
|---|---|---|
| **Read Text** | OCR on the full frame or a cropped zone → string | Read a meter, shelf display, or digital sign |
| **Read License Plate** | ALPR/ANPR: localize plate, decode string, optional watchlist match | Auto-open gate for employee plates; alert on unknowns |
| **Read Barcode** | Detect and decode QR / 1D / Data Matrix → payload string | Verify shipments on a loading dock camera |
| **Extract Color** | Dominant color of a zone or full frame → RGB / named color | Detect traffic light state; confirm high-vis vest present |
| **Estimate Headcount** | Density-map regression → integer count (stays accurate in crowds) | Alert when occupancy exceeds fire-code limit |

## Group 3 — Single-Frame Analysis

| Node | What it does | Camera use case |
|---|---|---|
| **Detect PPE** | Per-person compliance check: hard hat, vest, glasses, mask | Alert supervisor when a worker enters a zone without a hard hat |
| **Detect Fire/Smoke** | Flame/smoke classifier — faster and more specialized than a general VLM | Detect server room fire minutes before a heat sensor triggers |
| **Compare Frame** | Pixel/embedding similarity vs. a stored reference image → numeric score | Confirm a safe door is closed; alert when equipment was moved |
| **Detect Anomaly** | Scores how unusual the frame is vs. a learned baseline — no labeled classes | Flag anything unexpected in a production line or sterile room |

## Group 4 — Temporal / Cross-Frame

> Track Object is the foundational primitive; all others consume its ID stream with per-ID state.

| Node | What it does | Camera use case |
|---|---|---|
| **Track Object** | Assigns persistent IDs to detected objects across frames (ByteTrack/SORT) | Follow a person through multiple rooms without losing them |
| **Measure Dwell** | How long a tracked ID has been inside a defined zone → fires when threshold exceeded | Alert when a visitor lingers in a restricted area > 90 s |
| **Count Line Crossings** | Counts objects crossing a virtual line, per direction (in / out) | Real-time store occupancy; enforce fire-code limits |
| **Detect Direction** | Dominant movement direction → semantic label (toward/away, left/right, bearing) | Wrong-way alert in a one-way exit lane |
| **Detect Speed** | Real-world velocity via frame-rate + pixel-to-meter calibration | Alert when a vehicle exceeds 15 km/h in a pedestrian zone |
| **Detect Loitering** | Time in zone + low net displacement (slow/erratic) over threshold | Alert when someone circles an ATM entrance for > 2 minutes |
| **Detect Abandoned Object** | Object appears, owner departs, object stays for N seconds | Unattended-bag alert in a lobby or transit area |
| **Detect Tailgating** | Counts persons per access event; fires when > 1 on a single badge swipe | Alert when someone slips through a server-room door behind an authorized person |
| **Detect Scene Change** | Persistent baseline learning; fires on lasting deviations or camera tampering | Alert when a fire extinguisher is removed from its bracket |

## Group 5 — Safety Events

| Node | What it does | Camera use case |
|---|---|---|
| **Detect Fall** | Person rapidly goes from upright to horizontal and stays prone > N seconds | Alert nursing staff when a resident collapses in a corridor |
| **Detect Crowd** | Density or count exceeds threshold, or rapid group formation detected | Alert when a venue section approaches crush density |
| **Classify Activity** | N-frame clip → activity label: walking, running, fighting, crouching, hiding | Detect shoplifting concealment gesture or a fight on the shop floor |

## Group 6 — Identity / Multi-Camera

| Node | What it does | Camera use case |
|---|---|---|
| **Match Watchlist** | Face embedding or plate string vs. allow/deny list → `match` / `no match` exec | Grant access or alert security based on employee vs. flagged list |

---

## Implementation priority notes

Highest ROI for home + business security (non-technical users):
1. **AI Extract** — general VLM extraction, low effort (extends AI If pattern)
2. **Read License Plate** — concrete, high-value, self-contained
3. **Measure Dwell** — needs Track Object first; very high value for retail/office
4. **Count Line Crossings** — occupancy counting, universal use case
5. **Detect Fall** — safety vertical (care homes, warehouses)
6. **Detect Abandoned Object** — transit/lobby safety

Temporal group dependency: Track Object must ship before Measure Dwell, Count Line Crossings, Detect Direction, Detect Speed, Detect Loitering, Detect Tailgating, Detect Abandoned Object, Classify Activity.
