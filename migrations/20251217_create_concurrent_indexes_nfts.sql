-- ⚠️ WARNING: This migration must be run OUTSIDE of a transaction
-- Use: psql -h HOST -U USER -d DATABASE -f this_file.sql
-- Or run each CREATE INDEX CONCURRENTLY statement separately

-- Drop existing non-concurrent indexes first (if they exist)
DROP INDEX IF EXISTS idx_nfts_token_id;
DROP INDEX IF EXISTS idx_nfts_is_mint;
DROP INDEX IF EXISTS idx_nfts_user_address;
DROP INDEX IF EXISTS idx_nfts_user_received;
DROP INDEX IF EXISTS idx_nfts_received;

-- NFTs table concurrent indexes for better performance without table locks

-- Index for token_id lookups (used after minting)
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_nfts_token_id_concurrent 
ON nfts(token_id);

-- Index for is_mint status filtering (0: cannot mint, 1: applying, 2: minted)
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_nfts_is_mint_concurrent 
ON nfts(is_mint);

-- Index for user_address lookups (primary query pattern)
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_nfts_user_address_concurrent 
ON nfts(user_address);

-- Composite index for user + received status (most common query)
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_nfts_user_received_concurrent 
ON nfts(user_address, received);

-- Index for received status filtering
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_nfts_received_concurrent 
ON nfts(received);

-- Add comments to document the indexes
COMMENT ON INDEX idx_nfts_token_id_concurrent IS 'Fast lookup by token_id (concurrent)';
COMMENT ON INDEX idx_nfts_is_mint_concurrent IS 'Filter by mint status (concurrent)';
COMMENT ON INDEX idx_nfts_user_address_concurrent IS 'Fast lookup by user address (concurrent)';
COMMENT ON INDEX idx_nfts_user_received_concurrent IS 'Composite index for user + received queries (concurrent)';
COMMENT ON INDEX idx_nfts_received_concurrent IS 'Filter by received status (concurrent)';

-- Update statistics
ANALYZE nfts;

