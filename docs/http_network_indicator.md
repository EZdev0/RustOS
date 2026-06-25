# HTTP Networking and Status Indicator

## Overview
This document details the recent enhancements made to the Rust OS network stack and desktop compositor, primarily focusing on HTTP support via `smoltcp` and the desktop network indicator.

## Network Status Indicator
The top-right corner of the `GraphicalCompositor` top bar now displays a network status indicator.
- **[INET]** (Green) is displayed when `NetworkManager` is initialized and active.
- **[OFF]** (Red) is displayed if there is no active network configuration.

This allows the user to immediately see if the Ethernet connection and TCP/IP stack are up.

## Browser HTTP Implementation
The built-in `BrowserApp` has been updated to query `example.com` (93.184.216.34) over HTTP port 80.
- A `tcp::Socket` was added to the `NetworkManager`'s `SocketSet`.
- An `HttpState` state-machine was introduced (`Init`, `Connecting`, `SendingRequest`, `ReceivingResponse`, `Done`).
- `NetworkManager::poll` handles the non-blocking state progression, reading the response dynamically into `http_response`.
- `BrowserApp` initiates the request via `start_http_request()` when initialized, displaying "Loading...", and automatically updates its content when `get_http_response()` returns the payload.

## Taskbar Fixes
We also validated the glass dock taskbar icon limits. The dock calculation strictly respects `max_apps_possible = width.saturating_sub(40) / 85;`, preventing bottom bar overflow if the user opens or registers too many apps.
