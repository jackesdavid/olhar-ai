# What the platforms actually allow for an urgent notification

Type: research
Status: resolved

## Question

The ask is "all possible alarms — vibration, sound, whatever grabs attention". Establish what iOS
and Android really permit, so the plan promises only what can ship.

- iOS: time-sensitive vs **critical** alerts — critical pierces Do Not Disturb and needs a separate
  Apple entitlement. What does requesting it involve, and is it plausible for this app?
- Android: notification channels, importance levels, custom sound, whether DND can be bypassed and
  under what conditions.
- Expo specifically: what `expo-notifications` exposes of the above on each platform, and what needs
  a config plugin or a native change.
- Delivery reliability: what happens to a push while the phone is off, in Doze, or has the app
  force-quit — and how long the service will retry.

That last point matters beyond the notification: it is the evidence for whether a TTL of minutes is
realistic or whether the product should assume the phone is checked, not alerted.

AFK.

## Answer: time-sensitive on iOS, a high-importance channel on Android — and the TTL cannot assume the alert lands

### iOS

**Critical alerts are not available to us in practice.** The entitlement exists, but Apple restricts
it to healthcare and public-safety apps, reviews every request by hand over days to weeks, and
declines cases that are "merely time-sensitive" rather than life-safety. A gate-opening automation
is exactly the case they decline. Planning around it would be planning around something we cannot
ship.

What we can have is `interruptionLevel: 'timeSensitive'`, which expo-notifications exposes
directly. It breaks through Focus and the Scheduled Summary, needs the Time Sensitive Notifications
capability rather than a reviewed entitlement, and — importantly — **the user can revoke it per
app** in Settings. It does not pierce Do Not Disturb the way critical alerts do.

### Android

A dedicated channel at `AndroidImportance.MAX` with a custom sound and vibration pattern. Channels
are created at runtime, and the config plugin registers the sound file at build time. DND bypass is
possible via the channel, but only if the user grants the app Do Not Disturb access — a separate
system permission we can request and cannot assume.

### Delivery is best-effort, and that is the finding that matters

Neither platform guarantees the alert arrives, let alone is seen: a phone that is off, in Doze, or
force-quit may get the push late or not at all, and on iOS the user can silently revoke the
time-sensitive level.

**So the TTL must assume the phone is CHECKED, not that the person was ALERTED.** Two consequences,
both already in the baseline and now with evidence behind them rather than caution:

- Fetching pending requests on launch and on foreground (`GET /device-gate/pending`) is not a
  nice-to-have fallback — it is the reliable path, and the notification is the optimisation.
- A TTL measured in a couple of minutes will expire on a phone in a pocket. Minutes-to-hours is the
  realistic range, and the panel's default should reflect that rather than implying an alarm.

`addNotificationResponseReceivedListener` handles the tap and needs no native change.
