# Sovereign Notes

A CLI note-taking tool that syncs notes across your devices peer-to-peer. No server, no cloud — just your devices talking directly.

## How it works

Notes are stored as JSON files on disk. Three SwarmNL communication patterns work together:

- **Replication** (eventual consistency) — note metadata is replicated across a `sovereign-notes-sync` replica network
- **Gossip** — real-time announcements on `sovereign-notes-changes` topic when notes are created or updated
- **RPC** — on-demand full note content transfer between peers

## Install

From the `apps/sovereign-notes` directory:

```bash
cargo install --path .
```

This puts `sovereign-notes` on your PATH. Alternatively, use `cargo run --` in place of `sovereign-notes` for any command below.

## CLI Commands

```bash
# Create a note
sovereign-notes new "My Note Title"

# Edit a note's content
sovereign-notes edit <note-id> "Updated content here"

# List all notes
sovereign-notes ls

# Read a note
sovereign-notes read <note-id>

# Sync notes from connected peers
sovereign-notes sync

# Show network and sync status
sovereign-notes status
```

## Multi-device usage

Start on device 1:
```bash
sovereign-notes --tcp-port 51000 --udp-port 51001 new "Shopping List"
```

Note the PeerId, then on device 2:
```bash
sovereign-notes --tcp-port 51100 --udp-port 51101 \
  --boot-peer-id <PEER_ID> \
  --boot-addr /ip4/<DEVICE1_IP>/tcp/51000 \
  sync
```

## Testing

```bash
cargo test -- --test-threads=1
```

The integration test spawns two in-process nodes (ports 49200 and 49300), creates a note on node1, broadcasts it via gossip, and verifies node2 can fetch it via RPC.

## SwarmNL patterns used

| Pattern | Purpose |
|---------|---------|
| Replication | Eventual consistency for note metadata |
| Gossip | Real-time change announcements |
| RPC | On-demand note content transfer |
