# EutherLab JSON API v1

Run `./eutherlab api`. Send one JSON object per line on stdin and read one
JSON object per line on stdout. Operations are synchronous; requests on one
connection are sequential. Separate controller processes can safely operate
different VM names concurrently. No TCP listener, authentication service or
LLM is involved. Future MCP tools should call this interface with the same
workspace-scoped host permissions.

```json
{"version":1,"id":"b1","method":"boot","params":{"vm":"agent-a"}}
{"version":1,"id":"s1","method":"status","params":{"vm":"agent-a"}}
{"version":1,"id":"l1","method":"logs","params":{"vm":"agent-a"}}
{"version":1,"id":"d1","method":"debug","params":{"vm":"agent-a","registers":true,"probe":true}}
{"version":1,"id":"p1","method":"screenshot","params":{"vm":"agent-a"}}
{"version":1,"id":"r1","method":"reset","params":{"vm":"agent-a","fresh":true}}
{"version":1,"id":"x1","method":"stop","params":{"vm":"agent-a"}}
{"version":1,"id":"c1","method":"clean","params":{"vm":"agent-a"}}
```

Successful envelope:

```json
{"version":1,"id":"x1","ok":true,"result":{"vm":"agent-a","stopped":true,"was_alive":true}}
```

Errors use `ok:false` and `error:{"type":"ValueError","message":"..."}`.
The request ID is echoed where available. A malformed line returns an error
without terminating the server. Unknown methods, parameters and wrong value
types are rejected. CLI commands use the same result/error envelopes and a
nonzero exit code on error. A completed test with failed assertions has
`result.passed:false`; API callers must inspect that field, while CLI callers
also receive exit code 1.

| Method | Parameters | Result |
| --- | --- | --- |
| `build` | `eutherboot?`, `variant?` | Immutable build manifest and hashes |
| `boot` | `vm?`, `variant?`, `paused?`, `accel?`, `known_good?`, `build_id?` | VM resources, PID and QMP readiness |
| `status` | `vm?` | Process identity, QMP status and parsed kernel events |
| `logs` | `vm?` | Serial, EutherBoot and QEMU text (up to 4 MiB each) |
| `stop` | `vm?` | Verified VM stopped, logs retained |
| `reset` | `vm?`, `fresh?` | Warm reset or recreated VM from its original build |
| `screenshot` | `vm?` | PPM artifact path and SHA-256 |
| `debug` | `vm?`, `registers?`, `probe?` | GDB UNIX socket/command; optional QMP registers and GDB probe |
| `crash` | `vm?` | Logs, build/configuration and registers if still available |
| `test` | `suite?`, `timeout?`, `promote?` | Structured report; full suite needs all four variant builds |
| `clean` | `vm?` | Removed stopped VM runtime resources |

Default VM: `kernel-test`. Variant: `normal`, `panic`, `timeout` or `crash`.
Acceleration: `tcg` (default) or `kvm`. Suite: `smoke` (default) or `full`.
Timeout: 30 seconds by default, greater than zero and at most 600 seconds.
Boolean parameters are JSON booleans. VM names are 1–24 ASCII alphanumeric,
underscore or hyphen characters and must start alphanumeric.
`known_good` takes precedence over `build_id`, which takes precedence over
`variant`. Fresh reset always selects its original build ID.

An absent VM returns a structured error. `stop` is idempotent while the VM's
state exists; `clean` also succeeds for an already absent VM. Build state must
exist before boot. Every initial build invokes the Rust compiler and packages
an actual boot image; there is no mock execution mode.

The API provides no arbitrary command, QMP passthrough, physical-disk path,
network forwarding or host firmware operation. It is a trusted local control
interface, not an access-control boundary against other processes of the same
user. Do not expose stdin/stdout over a network without a separate security design.

The experimental `tasks` variant and `task0-low`, `task0-high`, `task1-low`,
`task1-high`, `task-overflow` fault variants are also accepted by build/boot.
`status` includes `task_events` parsed from `eutherus.tasks` version 1. See
[task bootstrap](TASKS-BOOTSTRAP.md) for event ordering and acceptance.

`graphics` now runs renderer/input tasks and reports real activity counters.
`graphics-input-exit` and `graphics-input-guard` inject fatal failures after both
tasks have made progress. A prior `boot_success` does not override a later
panic or CPU exception in `observe`/test results. See
[graphics bootstrap](GRAPHICS-BOOTSTRAP.md) for the telemetry fields.

Inspector diagnostics are carried in the `eutherus.graphics` serial stream:
`inspector_changed` records input selection/visibility, and `inspector_presented`
records the copied task snapshot actually rendered. `frame_presented` adds
`selected_task`, `inspector_visible` and `selection_revision`. The complete
field/observation contract is in [inspector](INSPECTOR.md).
