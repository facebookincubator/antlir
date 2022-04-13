---
id: update-flows
title: Update Flows
---

In the following diagrams, "Agent" is some on-host tool that calls
`metalctl {online,offline}-update` after determining the desired host state.
MetalOS then acts on the running system to arrive at the given host state.

## Online Update
This updates the `RuntimeConfig` (running versions of native services)

```mermaid
sequenceDiagram
Agent->>MetalOS: online-update stage
MetalOS->>Agent: packages downloaded
Agent->>MetalOS: online-update commit
MetalOS->MetalOS: evaluate service generators
MetalOS->MetalOS: manage state volumes
MetalOS->>systemd: link new service units
MetalOS->>systemd: start/stop services
systemd->>MetalOS: transaction results
MetalOS->>Agent: services success/failed
```

## Offline Update
This updates the `BootConfig` (primarily rootfs and kernel)

```mermaid
sequenceDiagram
Agent->>MetalOS: offline-update stage
MetalOS->>Agent: packages downloaded
Agent->>MetalOS: online-update commit
MetalOS->>Kernel: kexec
opt On failure
    MetalOS->>Agent: report failure
end
```
