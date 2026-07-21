# Performance & scale backlog

Improvements identified during development and two-phone hardware testing, ranked by impact.
None block current functionality; they matter for battery life and crowd-scale behavior.

## 1. Dedupe redundant links to the same peer — highest impact
Both phones advertise **and** scan **and** connect, so ~5 GATT connections form to the *same*
peer. Wastes battery (5 live connections vs 1), airtime (every broadcast is sent on all 5 links),
and reach (BLE phones hold only ~7–8 GATT links total, so 5 to one peer nearly prevents connecting
to a second person — this caps mesh density). Fix: once a peer's fingerprint is known from its
announce, keep one link per identity and drop the rest.

## 2. Use the negotiated MTU instead of the hardcoded 182
`requestMtu(517)` is called but the core is always told the usable payload is 182 (the iOS floor).
Android↔Android leaves ~3× headroom unused; larger frames mean fewer fragments (and the Noise
`msg2` currently straddles the fragment boundary at 182). Small, clean change.

## 3. Battery duty-cycling (M6)
Continuous `LOW_LATENCY` scan + advertise is the main battery drain (~10–15%/hr). Implement the
planned 4-tier adaptive scheme keyed to screen state, battery level, and recent activity.

## 4. Stop flooding directed messages mesh-wide
A DM is addressed to one peer but currently floods the whole mesh up to the TTL, though only the
recipient can decrypt. Fine for 2 phones, wasteful in a crowd. Move to source-routing / unicast
along a known path (falling back to flood).

## 5. Relay efficiency tuning
The 200-node simulator shows suppression trims only ~10% of rebroadcasts (~0.9/node vs the ~0.5N
aspiration). Add relay-election (~5% of nodes relay) and tune suppression against the simulator to
roughly halve mesh-wide airtime.

## Smaller
- App ticks on a fixed 120 ms timer; use the core's suggested next-wake to cut idle CPU/battery.
- GATT operation queue so the link-up announce write doesn't race the CCCD write (reliability).
- `poll_events` allocates a Vec each tick; negligible today.
