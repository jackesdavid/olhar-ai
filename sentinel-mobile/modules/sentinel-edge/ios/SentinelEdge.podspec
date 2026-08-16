Pod::Spec.new do |s|
  s.name           = 'SentinelEdge'
  s.version        = '1.0.0'
  s.summary        = 'Embedded sentinel-edge (Rust) for the phone-as-edge (card #144).'
  s.license        = 'MIT'
  s.author         = 'Sentinel'
  s.homepage       = 'https://github.com/olharai/Sentinel'
  s.platforms      = { :ios => '15.0' }
  s.swift_version  = '5.9'
  s.source         = { git: '' }
  s.static_framework = true

  s.dependency 'ExpoModulesCore'

  # The Rust edge, built by build-ios-lib.sh into ios/libs/. Static: its symbols (start_edge/…) link
  # straight into the app binary.
  s.vendored_libraries = 'libs/libsentinel_edge.a'
  s.source_files = '*.swift'

  # System libraries the Rust std + deps reference on iOS. If a device link fails with an undefined
  # symbol, the missing framework/lib is named there — add it here.
  # GStreamer + the system frameworks its plugins reference. VideoToolbox/CoreMedia/CoreVideo/
  # AudioToolbox/AVFoundation are what the applemedia plugin (HW decode/encode) needs; the rest is the
  # GStreamer-iOS standard set. Added as a block to avoid iterating one missing symbol at a time.
  s.frameworks = 'Security', 'SystemConfiguration', 'CoreFoundation', 'GStreamer',
                 'VideoToolbox', 'CoreMedia', 'CoreVideo', 'AudioToolbox', 'AVFoundation',
                 'CoreGraphics', 'QuartzCore', 'OpenGLES', 'MediaToolbox', 'CoreAudio', 'AssetsLibrary'
  s.libraries  = 'c++', 'resolv', 'iconv'

  # The GStreamer iOS framework is a STATIC ar archive (linked into the app binary, not embedded),
  # so the app just needs -framework GStreamer + where to find it. It lives in the SDK, not a standard
  # framework path, hence the explicit search path on BOTH the pod and the consuming app target — the
  # app is where the vendored .a's gst/glib symbols must resolve (card #144).
  # TWO search paths, and the order is the point (#185).
  #
  # A CI worker has no `$HOME/Library/Developer/GStreamer` — that is a manual SDK install on a
  # developer's Mac. An EAS build therefore failed at link with `framework 'GStreamer' not found`,
  # four steps past the error anyone reads, and only after compiling everything.
  #
  # So the vendored path comes FIRST: `scripts/fetch-edge-lib.sh` drops an arm64-only GStreamer
  # there on a fresh checkout, and a machine that has it locally never downloads anything. The
  # `$HOME` path stays as the fallback so an existing developer setup keeps working untouched.
  # TWO DIFFERENT VARIABLES, because the same one does not resolve in both places.
  # `PODS_TARGET_SRCROOT` is defined for the POD target only; in the consuming app target it
  # expands to nothing and the path collapses to a bare "/libs" — which the linker reported as
  # `search path '/libs' not found` and I read past twice. The app target has `SRCROOT` = ios/,
  # so it walks up to the module directory instead.
  home_sdk = '"$(HOME)/Library/Developer/GStreamer/iPhone.sdk"'
  pod_gst  = %Q{$(inherited) "$(PODS_TARGET_SRCROOT)/libs" #{home_sdk}}
  app_gst  = %Q{$(inherited) "$(SRCROOT)/../modules/sentinel-edge/ios/libs" #{home_sdk}}
  s.pod_target_xcconfig = {
    'DEFINES_MODULE' => 'YES',
    'FRAMEWORK_SEARCH_PATHS' => pod_gst
  }
  s.user_target_xcconfig = {
    'FRAMEWORK_SEARCH_PATHS' => app_gst
  }
end
