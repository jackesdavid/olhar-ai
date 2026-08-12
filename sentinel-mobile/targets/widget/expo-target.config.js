/** @type {import('@bacons/apple-targets/app.plugin').Config} */
module.exports = {
  type: "widget",
  name: "Olhar AI",
  icon: "../../assets/icon.png",
  // The App Group is the ONLY channel between the app and the widget. A widget extension is a
  // separate process with its own sandbox: it cannot read the app's SecureStore, and it must not
  // hold a session token of its own. The app writes {id, name} pairs here; the widget draws
  // buttons from them — no credentials ever cross.
  entitlements: {
    "com.apple.security.application-groups": ["group.com.olharai.app"],
  },
};
