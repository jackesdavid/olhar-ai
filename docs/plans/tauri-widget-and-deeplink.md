# The widget in the Tauri shell — and the write path nobody sees

**Board card:** #203 Widget + deep link in the Tauri shell
**Goal:** Keep the one-tap-workflows widget working when the phone app becomes the Tauri shell, and
have it open **that** app.
**Depends on:** #202 (the shell itself). **Blocks:** retiring `sentinel-mobile`.

## What exists today

`sentinel-mobile/targets/widget/OlharAIWidget.swift` — 234 lines of SwiftUI. It reads pinned
workflows out of an App Group and each button deep-links into the app:

```swift
static let suite = "group.com.olharai.app"   // App Group
static let key   = "workflows"
Link(destination: URL(string: "olharai://run/\(w.id)")!)
```

`sentinel-mobile/src/pinned.ts` is what **writes** that container. The scheme `olharai` is registered
by the Expo config, and the widget bundle is `com.olharai.app.widget`.

## The rule that decides the shape

**A widget launches the app it is embedded in.** WidgetKit accepts SwiftUI only — not even UIKit,
and certainly not a web view, because a widget running a browser would be killed for power use. So
the widget was never shared code and never can be. Moving it is not a port; it is re-hosting the
same Swift in a different bundle.

## Three pieces

### 1. The target
Add a widget extension to the Tauri-generated Xcode project and enable the App Group on both it and
the app. **The Swift does not change.**

### 2. The deep link
Register `olharai` in the shell's Info.plist, take `tauri-plugin-deep-link` to receive
`olharai://run/<id>`, and route it in SvelteKit. Ordinary work.

### 3. The write path — the one that is easy to miss
`pinned.ts` writes `UserDefaults` today because React Native can. **The web app inside the shell
cannot**: it is JavaScript in a webview with no access to a shared container. A Rust command in the
shell has to write the App Group, and the UI has to call it when a workflow is pinned.

Skip this and the widget installs, opens the right app, and is **permanently empty** — which looks
like a broken widget rather than a missing write path.

## The regeneration problem, to settle BEFORE building

`src-tauri/gen/` is gitignored because `tauri ios init` regenerates it. A target added by hand there
disappears on the next regeneration. Two ways out, and the choice is a real one:

- **Version the generated project.** Simple, but `gen/` stops being disposable and starts being
  something to merge.
- **Automate the insertion.** A script that adds the target after generation. More work, keeps
  `gen/` throwaway, and is the only version CI can reproduce — and every build must happen in CI.

Expo does this declaratively today (`targets/widget` survives everything), so this is a capability
being **given up**, not gained. Worth saying plainly.

## Verification criteria

1. The widget appears, and tapping a workflow opens the **Tauri** app, not the Expo one.
2. `olharai://run/<id>` lands on the right screen and runs that workflow.
3. Pinning a workflow in the web UI updates the widget — proving the Rust write path.
4. A clean checkout plus a CI build produces an app **with** the widget, without hand steps.

Criterion 4 is the one that decides whether the regeneration answer was good enough.

## Out of scope
- Android widgets. iOS first.
- Live Activities.
