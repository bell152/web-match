-- Create swap_requests table
CREATE TABLE IF NOT EXISTS swap_requests (
    id                      BIGSERIAL PRIMARY KEY,

    user_address            VARCHAR(42) NOT NULL,
    zero_for_one            BOOLEAN NOT NULL,

    amount_in_raw           NUMERIC(78,0) NOT NULL,
    amount_out_raw          NUMERIC(78,0) NOT NULL,

    token_decimals          INTEGER NOT NULL DEFAULT 18,

    block_timestamp_raw     BIGINT NOT NULL,
    timestamp_utc           TIMESTAMPTZ NOT NULL,

    created_at              TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_swap_user_address ON swap_requests(user_address);
