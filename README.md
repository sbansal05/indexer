# Solana Token Transfer Indexer

**A real-time SPL Token transfer feed built on Yellowstone gRPC — the production data layer TrustTrail's wallet-history lookups actually need.**

TrustTrail currently reconstructs a wallet's on-chain history by polling Helius: asking "what's changed?" over and over, on demand, per lookup. That works for a demo. It doesn't scale, and it can't tell you about something the moment it happens — only the next time you happen to ask. This project is the alternative: a client that opens a standing connection to a Solana validator's event stream and is told the instant a token transfer happens, rather than asking.

It's deliberately small. Scope is the point, not a limitation:

- SPL Token transfers only — not swaps, not a DEX, not NFTs
- A local devnet validator with a self-generated traffic stream — not mainnet, not historical backfill
- No frontend — this is a data-layer proof, not a product

---

**The whole point of this project, in four screenshots:** kill the indexer mid-stream, restart it, and watch it pick up exactly where it left off — zero gaps, zero duplicate rows.

Running fresh, decoding real transfers — the banner correctly reads "Fresh start" right after a clean truncate:
<img src="docs/demo-1-running.png" width="700">
<br>
23 rows before a restart:
<br><br>
<img src="docs/demo-2-before-count.png" width="700">
<br><br>
Killed and restarted — the banner reads a real slot straight from Postgres, not "from now":
<br>
<img src="docs/demo-3-resume.png" width="700">
<br>
170 rows after — kept climbing from where it left off, not reset:
<br>
<img src="docs/demo-4-after-count.png" width="700">

---

## The problem this solves, one level down

Solana validators know about every account change and transaction the instant they process it — it's literally what they're doing. Geyser is the hook built into validator software that exposes that internal firehose. Yellowstone is the standard way of putting that firehose on the network as a gRPC stream, so any external program can open a connection and start receiving events live, instead of asking a validator "anything new?" over and over.

That live connection is the whole value proposition — and also the whole engineering problem. Connections drop: networks hiccup, validators restart, your own process crashes. For most apps that's a shrug. For an indexer, whose entire job is *completeness*, a dropped connection means a silent, permanent gap — transfers nobody told you about and that you have no way to retroactively discover.

## Architecture

![Architecture diagram](docs/architecture.png)

## Components

### 1. Local validator + Geyser plugin
A single-node, ephemeral, fully local Solana cluster (`agave-test-validator`) with the open-source `yellowstone-grpc-geyser` plugin compiled and loaded via `--geyser-plugin-config`. Its only activity is whatever the traffic generator creates on it.

It's a private simulation, not a connection to the real public Solana devnet — "devnet" here just means "not real money," not "the shared public network."

It plays the role a hosted provider like QuickNode would play in production — but every managed Yellowstone provider turns out to be mainnet-only. Chainstack's docs say so outright, and every QuickNode endpoint example uses `solana-mainnet.quiknode.pro`, with no devnet equivalent shown anywhere. TrustTrail itself is deployed on devnet, so this project isn't a claim that it watches TrustTrail's live activity today. The accurate framing: this is the pipeline TrustTrail's data layer would run on once that lending activity is tracked on mainnet.

The client code itself doesn't know or care which network it's pointed at — it's the standard `yellowstone-grpc-client` crate speaking the standard Dragon's Mouth protocol, unchanged whether the endpoint is `localhost` or QuickNode's mainnet address with an `x-token` header added.

### 2. Traffic generator (`scripts/traffic-gen/`, TypeScript)
The local validator starts with an empty ledger — there's nothing to stream until something happens. This is a small, separate script that creates a test mint, funds a handful of keypairs, and loops submitting `Transfer`/`TransferChecked` instructions between them on an interval. It is explicitly a dev fixture, not part of the deliverable, and lives in its own folder so nobody mistakes it for "the indexer."

### 3. Connection module (`grpc.rs`)
Opens the gRPC channel and builds the `SubscribeRequest`: a `transactions` filter with `account_include` set to the classic Token Program ID, commitment level `Confirmed`. Returns the stream of `SubscribeUpdate` messages. This is the literal phone line everything else depends on.

### 4. Reconnection policy (client config)
`yellowstone-grpc-client` ships its own `ReconnectionPolicy` — automatic reconnect with exponential backoff, on by default, including correct handling of equivocation (a validator briefly producing two versions of the same slot before one finalizes) via blockhash comparison. We configure this rather than write a retry loop ourselves, since re-deriving the equivocation handling by hand is easy to get subtly wrong.

This only covers one failure mode, though: a transient drop *while the process keeps running*.

### 5. Watermark / crash recovery (`db.rs`) 
The reconnection policy above does nothing if the process itself dies — a crash, a redeploy, being stopped overnight — because a fresh process has no in-memory state to reconnect from. This module persists `last_committed_slot` to Postgres after every successful write, and reads it back on startup to set `from_slot` on the very first subscribe request of a new process. The library gets you through a hiccup; this is what gets you through a restart.

### 6. Decode layer (`decode.rs`)
Each raw transaction is turned into a list of `InstructionUpdate` values via `InstructionUpdate::build_from_txn` — a standalone function in `shipstern-core`, needing no framework `Runtime`, which is what confirmed the hand-rolled architecture was viable. Each one is then handed to Shipstern's `InstructionParser`, keeping only `Transfer` / `TransferChecked` variants — everything else (mints, burns, approvals) is dropped here. Conceptually the same work as manually deriving Kamino's instruction discriminators in TrustTrail, just for a program where someone has already done the byte-layout work.

### 7. Idempotent writer (`db.rs`)
`INSERT ... ON CONFLICT (slot, signature, instruction_index) DO NOTHING` into `token_transfers`, in the same transaction as the watermark update. This is what makes replay-after-reconnect safe: seeing the same transfer twice becomes a harmless no-op instead of a duplicate row or a crash.

### 8. Schema
```sql
CREATE TABLE token_transfers (
    slot                BIGINT NOT NULL,
    signature           TEXT NOT NULL,
    instruction_index   SMALLINT NOT NULL,
    source_pubkey       TEXT NOT NULL,
    destination_pubkey  TEXT NOT NULL,
    authority_pubkey    TEXT NOT NULL,
    mint                TEXT,              -- NULL for legacy Transfer, only TransferChecked carries a mint
    amount              NUMERIC NOT NULL,
    decimals            SMALLINT,          -- NULL for legacy Transfer
    received_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (slot, signature, instruction_index)
);

CREATE TABLE indexer_watermark (
    id                  SMALLINT PRIMARY KEY DEFAULT 1,
    last_committed_slot BIGINT NOT NULL,
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

## Known limitations

- Replay recovers from a brief disconnect, not an extended outage — Yellowstone's replay buffer covers roughly the last 3,000 slots, about 20 minutes, not unlimited history.
- `mint` and `decimals` are nullable because the legacy `Transfer` instruction doesn't carry a mint account at all — that's a property of the instruction format, not a gap in the indexer.
- No block timestamps. A real `block_time` requires a second subscription (`SubscribeUpdateBlockMeta`) joined on slot — out of scope for a narrow build; `received_at` (wall-clock at insert time) is stored instead.
- Multisig transfers aren't captured distinctly — both account structs carry a `multisig_signers: Vec<Pubkey>` field for SPL Token's multisig-authority feature, out of scope for this narrow build; only the single `owner` is stored as `authority_pubkey`.
- Block reconstruction has a documented bug on non-leader validators like this local one (zero entry counts) — doesn't affect this project, since only transaction-level Token instructions are used, not full block metadata.

## Running locally

Needs: Rust/Cargo, the Agave CLI (`agave-install`), Node.js/npm, and PostgreSQL, all installed and on `PATH`.

**1. Build the Geyser plugin** — this lives outside this repo, in its own clone:
```bash
git clone https://github.com/rpcpool/yellowstone-grpc.git ~/tools/yellowstone-grpc
cd ~/tools/yellowstone-grpc
cargo build --release -p yellowstone-grpc-geyser
```

**2. Match the Agave version the plugin was actually built against** — check the plugin's `Cargo.lock` for the pinned `agave-geyser-plugin-interface` version (this project needed `4.2.2` specifically; a mismatch here causes a cryptic pubkey-parsing panic on startup, not a helpful error):
```bash
agave-install init 4.2.2 --no-modify-path
```

**3. Point the config at your build** — `validator-config/geyser-config.json` is already in this repo; edit its `libpath` field to the absolute path of the `.so` you just built (e.g. `~/tools/yellowstone-grpc/target/release/libyellowstone_grpc_geyser.so`). Validate it before trusting it:
```bash
cd ~/tools/yellowstone-grpc
cargo run --bin config-check -- --config /path/to/this/repo/validator-config/geyser-config.json
```

**4. Set up Postgres** — create a role and database, then run the migration:
```bash
sudo service postgresql start
sudo -u postgres psql -c "CREATE USER indexer WITH PASSWORD 'devpassword';"
sudo -u postgres psql -c "CREATE DATABASE indexer_db OWNER indexer;"
PGPASSWORD=devpassword psql -h 127.0.0.1 -U indexer -d indexer_db -f migrations/0001_init.sql
```

**5. Create `.env`** in this repo's root (see `.env.example` for the shape):
```bash
echo 'DATABASE_URL=postgres://indexer:devpassword@127.0.0.1:5432/indexer_db?sslmode=disable' > .env
```

**6. Start the local validator with the plugin loaded** — leave this running in its own terminal:
```bash
solana-test-validator \
  --ledger ~/ledger-local \
  --geyser-plugin-config /path/to/this/repo/validator-config/geyser-config.json \
  --rpc-port 8899 \
  --dynamic-port-range 8000-8026
```

**7. Start the traffic generator** — in a second terminal, also left running:
```bash
cd scripts/traffic-gen
npm install
npx tsx index.ts
```

**8. Build and run the indexer** — in a third terminal:
```bash
cargo build
cargo run
```

You should see leveled log output (`INFO`, `WARN`) as it connects, subscribes, and starts decoding real transfers from the traffic generator. To see the resume behavior for yourself: let it run for a bit, `Ctrl+C` it, then `cargo run` again — the first line should read `Watermark on startup: Some(<a real slot>)`, not `None`, and row counts in `token_transfers` keep climbing from where they left off rather than resetting.

## Stack

Rust · `yellowstone-grpc-client` · `shipstern` (SPL Token parser, formerly yellowstone-vixen) · Postgres · `agave-test-validator` · TypeScript (traffic generator, dev-only)
