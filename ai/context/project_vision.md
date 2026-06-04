# Project Vision

## Project Name

Bishop — Weyland-Yutani Surveillance Interface (codename: bishop)

---

## Problem Statement

Operators monitoring a site through IP cameras have to juggle several disconnected
tools: a camera app to view feeds, a separate way to discover devices on the network,
spreadsheets or memory to track which camera covers which room, and yet another channel
to alert people when something happens. There is no single, fast, low-friction console
that ties live cameras, a spatial floor plan, an AI assistant, and outbound alerting
into one operator experience.

Bishop solves this by being a single retro-themed terminal console (styled after the
Weyland-Yutani / "MU-TH-UR" synthetic Bishop) where one operator can discover cameras,
watch live feeds, lay out a floor plan, talk to an AI synthetic ("Bishop") that can see
the cameras and act, and push alerts to paired Telegram beacons — all from the keyboard.

It matters because situational awareness during an incident depends on speed and
consolidation: every context switch between tools is lost time.

---

## Users

- **Operator** — the single person at the console monitoring a site. Primary user.
- **Alert recipient (beacon owner)** — a person paired to receive Telegram notifications;
  does not touch the console directly.
- **Installer / power operator** — the same operator wearing a setup hat: discovers
  cameras, validates credentials, builds the floor plan.

---

## Core Features

- Boot sequence + retro CRT presentation (identity / atmosphere)
- Camera discovery (ONVIF) and device registry (`devices.json`)
- Device detail: snapshot capture, credential test, rename
- Live RTSP stream playback (via ffmpeg) rendered to the terminal
- AI chat with "Bishop" (Claude) + tool use over the operator's own state
- Vision: AI describes a camera snapshot
- Voice synthesis (ElevenLabs TTS) and speech-to-text (ElevenLabs Scribe)
- Floor-plan map editor: rooms + camera placement (`map.json`)
- Alerts registry: create / list / deliver, with delivery records (`alerts.json`)
- Telegram notifications: secure pairing + beacons management (`notifications.json`)
- Configuration screen: audio mix, typing speed, RTSP transport, CRT sweep (`config.json`)

---

## Constraints

- **Single-binary Rust app**, terminal-first (ratatui), optional self-rendered CRT
  window (wgpu). Must still run `--tty` (no GPU) and `--no-default-features` (no audio).
- **Single operator, local-first.** State lives in flat JSON files at the repo root, not
  a database.
- **Non-blocking UI.** All external IO (Claude, ElevenLabs, Telegram, ONVIF, ffmpeg)
  happens on background threads communicating over mpsc channels.
- Secrets come from environment / `.env`, never committed.
- The Weyland-Yutani / Bishop aesthetic is a hard constraint, not decoration — it is part
  of the product identity.

---

## Success Criteria

- Operator can go from cold start to a live, named, floor-plan-placed camera without
  leaving the console.
- Bishop (AI) can answer "what's happening on camera X" and raise/deliver an alert in one
  conversation.
- Each new capability ships as a small vertical slice (one feature folder) without
  destabilizing existing screens.
- The app keeps running headless/tty for constrained environments.
