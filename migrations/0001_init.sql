CREATE TABLE token_transfers (
    slot                BIGINT NOT NULL,
    signature           TEXT NOT NULL,
    instruction_index   SMALLINT NOT NULL,
    source_pubkey       TEXT NOT NULL,
    destination_pubkey  TEXT NOT NULL,
    authority_pubkey    TEXT NOT NULL,
    mint                TEXT,
    amount              NUMERIC NOT NULL,
    decimals            SMALLINT,
    received_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (slot, signature, instruction_index)
);

-- Deliberately not seeding a row here. "No row yet" means "never run
-- before" — the app treats that as "start fresh, no from_slot." The
-- first successful write inserts the row. Seeding a fake starting
-- value (like slot 0) would be indistinguishable from a real slot 0
-- ever having been processed, which is exactly the kind of ambiguity
-- a watermark can't afford to have.
CREATE TABLE indexer_watermark (
    id                  SMALLINT PRIMARY KEY DEFAULT 1,
    last_committed_slot BIGINT NOT NULL,
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);
