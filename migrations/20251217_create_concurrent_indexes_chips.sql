-- ⚠️ WARNING: This migration must be run OUTSIDE of a transaction
-- Use: psql -h HOST -U USER -d DATABASE -f this_file.sql
-- Or run each CREATE INDEX CONCURRENTLY statement separately

-- Drop existing non-concurrent indexes first (if they exist)
DROP INDEX IF EXISTS idx_chips_nft_id;
DROP INDEX IF EXISTS idx_chips_user_address;
DROP INDEX IF EXISTS idx_chips_nft_user_received;
DROP INDEX IF EXISTS idx_chips_received;

-- Chips table concurrent indexes for query_mint optimization

-- Index on nft_id for fast lookups of chips belonging to a specific NFT
-- Critical for counting total chips per NFT
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_chips_nft_id_concurrent 
ON chips(nft_id);

-- Index on user_address for fast lookups of chips owned by a user
-- Used in user chip queries
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_chips_user_address_concurrent 
ON chips(user_address);

-- Composite index on (nft_id, user_address, received) - MOST IMPORTANT
-- Covers the exact query pattern: WHERE nft_id = $1 AND user_address = $2 AND received = true
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_chips_nft_user_received_concurrent 
ON chips(nft_id, user_address, received);

-- Index on received flag for filtering received/unreceived chips
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_chips_received_concurrent 
ON chips(received);

-- Additional index on file_name for tile lookups
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_chips_file_name_concurrent 
ON chips(file_name) WHERE file_name IS NOT NULL;

-- Add comments to document the indexes
COMMENT ON INDEX idx_chips_nft_id_concurrent IS 'Fast lookup of chips by NFT ID (concurrent)';
COMMENT ON INDEX idx_chips_user_address_concurrent IS 'Fast lookup of chips by user address (concurrent)';
COMMENT ON INDEX idx_chips_nft_user_received_concurrent IS 'Composite index for mint eligibility queries (concurrent)';
COMMENT ON INDEX idx_chips_received_concurrent IS 'Filter chips by received status (concurrent)';
COMMENT ON INDEX idx_chips_file_name_concurrent IS 'Partial index for file_name lookups (concurrent)';

-- Update statistics
ANALYZE chips;

