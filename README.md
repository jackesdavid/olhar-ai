# BISHOP/AP

A retro Weyland-Yutani terminal assistant: a Nostromo-style boot sequence that
hands off to a live "motion tracker" sonar and a Bishop-persona chat loop.
Built with ratatui.

## Run

```sh
cargo run --release            # with audio (default)
cargo run --release --no-default-features   # silent, no audio dependency
```

Best viewed inside **cool-retro-term** for the CRT/phosphor glow.

## What happens

1. **Boot** — `MU/TH/UR 6000` self-test scrolls line by line (beep per line),
   ending on `INTERFACE 2037 READY FOR INQUIRY`. Any key skips it.
2. **Interface** — header, live motion-tracker sonar, tracking readout,
   message log, and an inquiry input bar. Type, Enter to send, Esc to quit.

## Audio

Optional, behind the `audio` feature (on by default). Put WAV/OGG files in
`assets/` — see `assets/README.txt` for the filenames and license-clean
sources. No device or no files = runs silently, never panics.

## Make it a real assistant

- `bishop_reply()` in `src/main.rs` is an offline stub — swap it for an LLM
  call. Do the call off-thread and push the reply into `app.log` via a channel
  so the sonar keeps sweeping while Bishop "thinks".
- `app.blips` is simulated in `on_tick()` — feed real signals (events, unread
  counts, etc.) by mapping each to an `(angle, radius)`; rendering is unchanged.
