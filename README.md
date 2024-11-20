# esp32c3-embassy-wifi-http

An embassy example to access a web site over WiFi from the ESP32-C3.
The web site is accessed and printed to the terminal when
a button is pressed.

The goals are:
- Only use crates that are in `crates.io` (no crates loaded from a github repository)
- Use, as far as possible, the latest version of the crates.
- Use embassy features to explore the use of async functions.
- Use the `reqwless` crate to handle the HTTP connection.

Notes:
- Currently cannot use the latest version of `reqwless` due to [this issue](https://github.com/drogue-iot/reqwless/issues/93).
- The version numbers of crates in the embassy and esp-hal area change quickly so no guarantee can be given that
  the dependencies used actually use the latest version.
- For the embassy-executor the default version of the "feature" `task-arena-size` was too small.

License: MIT
