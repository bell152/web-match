-- ⚠️ WARNING: This migration must be run OUTSIDE of a transaction
-- Use: psql -h HOST -U USER -d DATABASE -f this_file.sql
-- Or run each CREATE INDEX CONCURRENTLY statement separately

-- Create composite index for querying minted NFTs (is_mint = 2 AND received = true)
-- This is optimized for the new endpoint that queries all successfully minted NFTs
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_nfts_is_mint_received_concurrent 
ON nfts(is_mint, received);

-- Add comment to document the index purpose
COMMENT ON INDEX idx_nfts_is_mint_received_concurrent IS 'Composite index for querying minted NFTs (is_mint=2, received=true)';

-- Update statistics
ANALYZE nfts;


