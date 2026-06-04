# Personas

## Persona 1

### Name

Vasquez — the Operator (on console)

### Goals

- See, at a glance, which cameras are online and what each room looks like right now
- Get a fast answer to "what is happening at camera X?" without leaving the keyboard
- Raise and deliver an alert the moment something matters

### Pain Points

- Switching between a camera app, a network scanner, and a chat tool loses critical seconds
- Hard to remember which physical camera covers which room
- Mouse-driven surveillance UIs are slow under pressure

### Main Workflows

- Discover cameras, validate credentials, name them
- Watch a live feed; ask Bishop to describe a snapshot
- Lay out rooms on the floor plan and place cameras
- Create an alert and deliver it to beacons

---

## Persona 2

### Name

Hicks — the Beacon Owner (off console)

### Goals

- Be reachable instantly when the operator raises an alert
- Receive enough context (message + snapshot) to act without calling back

### Pain Points

- Generic notifications with no visual context
- Being paired insecurely / receiving alerts meant for someone else

### Main Workflows

- Pair to the bot once via `/pair <code>` in Telegram
- Receive alert messages and camera snapshots
- (Operator side) be enabled/disabled and renamed as a beacon

---

## Persona 3

### Name

Bishop — the AI Synthetic (in-app actor, not a human)

### Goals

- Use its tools to answer operator questions about the cameras and state
- Take safe actions on request: snapshot, describe, notify, create/deliver alerts

### Pain Points (design constraints it operates under)

- Must act only through the registered tool surface, never by touching files directly
- Must keep responses in-character and concise for the teletype reveal

### Main Workflows

- list_devices / get_camera_snapshot / describe_camera / show_live_feed
- send_notification / create_alert / list_alerts / alert_stats / deliver_alert
