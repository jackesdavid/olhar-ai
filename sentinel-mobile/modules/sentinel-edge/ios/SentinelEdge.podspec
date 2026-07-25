Pod::Spec.new do |s|
  s.name           = 'SentinelEdge'
  s.version        = '1.0.0'
  s.summary        = 'Embedded sentinel-edge (Rust) for the phone-as-edge (card #144).'
  s.license        = 'MIT'
  s.author         = 'Sentinel'
  s.homepage       = 'https://github.com/jackesdavid/Sentinel'
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
  s.frameworks = 'Security', 'SystemConfiguration', 'CoreFoundation', 'GStreamer'
  s.libraries  = 'c++', 'resolv'

  # The GStreamer iOS framework is a STATIC ar archive (linked into the app binary, not embedded),
  # so the app just needs -framework GStreamer + where to find it. It lives in the SDK, not a standard
  # framework path, hence the explicit search path on BOTH the pod and the consuming app target — the
  # app is where the vendored .a's gst/glib symbols must resolve (card #144).
  gst_search = '$(inherited) "$(HOME)/Library/Developer/GStreamer/iPhone.sdk"'
  s.pod_target_xcconfig = {
    'DEFINES_MODULE' => 'YES',
    'FRAMEWORK_SEARCH_PATHS' => gst_search
  }
  s.user_target_xcconfig = {
    'FRAMEWORK_SEARCH_PATHS' => gst_search
  }
end
