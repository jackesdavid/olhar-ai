# What the platforms actually allow for an urgent notification

Type: research
Status: open

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
