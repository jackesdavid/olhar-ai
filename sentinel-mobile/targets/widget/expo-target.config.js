/** @type {import('@bacons/apple-targets/app.plugin').Config} */
module.exports = {
  type: "widget",
  // NOT "Olhar AI". Sanitised, that becomes "OlharAI" — the app target's own name — and the two
  // collide when credentials are keyed by target: the widget's provisioning profile overwrites the
  // app's, and the build fails with "profile has app ID com.olharai.app.widget, which does not
  // match the bundle ID com.olharai.app". The user-visible name is set in Swift
  // (`configurationDisplayName`), so this string is internal and free to be unambiguous.
  name: "OlharAIWidget",
  icon: "../../assets/icon.png",
  // 17.0 because the widget is CONFIGURABLE: `AppIntentConfiguration` and `AppEntity` are the iOS 17
  // way to let someone long-press → Edit Widget → pick which workflow this button runs. The older
  // `IntentConfiguration` needed a SiriKit intent definition file, which prebuild does not generate.
  deploymentTarget: "17.0",
  // The App Group is the ONLY channel between the app and the widget. A widget extension is a
  // separate process with its own sandbox: it cannot read the app's SecureStore, and it must not
  // hold a session token of its own. The app writes {id, name} pairs here — the picker reads them
  // to offer choices, and the button reads them to draw a label. No credentials ever cross.
  entitlements: {
    "com.apple.security.application-groups": ["group.com.olharai.app"],
  },
};
