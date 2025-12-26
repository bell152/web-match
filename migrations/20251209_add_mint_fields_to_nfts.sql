-- Add token_id, is_mint, and block_number columns to nfts table
-- is_mint states: 0 = cannot mint (default), 1 = can apply for mint, 2 = mint successful

ALTER TABLE nfts 
ADD COLUMN token_id BIGINT,
ADD COLUMN is_mint INTEGER NOT NULL DEFAULT 0 CHECK (is_mint IN (0, 1, 2)),
ADD COLUMN block_number BIGINT;

-- Add index for token_id for faster lookups
CREATE INDEX IF NOT EXISTS idx_nfts_token_id ON nfts(token_id);

-- Add index for is_mint to efficiently query mintable NFTs
CREATE INDEX IF NOT EXISTS idx_nfts_is_mint ON nfts(is_mint);

-- Add comment to document is_mint states
COMMENT ON COLUMN nfts.is_mint IS '0: cannot mint, 1: can apply for mint, 2: mint successful';
