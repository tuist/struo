---
title: Info.plist files match Xcode and Bazel's Apple rules
date: 2026-11-26
---

- Xcode app targets with `GENERATE_INFOPLIST_FILE = YES` now get every
  `INFOPLIST_KEY_*` build setting in their Info.plist, from the project or an
  `.xcconfig`, typed as Xcode types it. `UILaunchScreen_Generation` and
  `UIApplicationSceneManifest_Generation` produce the dictionaries Xcode
  writes, so iOS apps no longer launch letterboxed.
- When such a target also sets `INFOPLIST_FILE`, that file is the starting
  point and the generated keys merge into it, as in Xcode. Version keys come
  from `CURRENT_PROJECT_VERSION` and `MARKETING_VERSION`, and are left out
  when those are unset.
- watchOS apps get `WKApplication`, and `UIDeviceFamily` covers TV, Watch,
  and Vision devices. `LSRequiresIPhoneOS` is added only to generated iOS
  and tvOS app plists from Xcode projects.
- `apple_application` gains `info_plist_entries` for typed Info.plist keys,
  plus `build_version` and `short_version_string`. `families` accepts `tv`,
  `watch`, `vision`, and `mac`, and defaults to the platform's family.
