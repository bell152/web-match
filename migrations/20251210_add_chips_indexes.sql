-- Add performance indexes to chips table for query_mint optimization
-- This migration adds indexes to improve the performance of mint eligibility queries

-- Index on nft_id for fast lookups of chips belonging to a specific NFT
-- This is crucial for counting total chips per NFT
CREATE INDEX IF NOT EXISTS idx_chips_nft_id ON chips(nft_id);

-- Index on user_address for fast lookups of chips owned by a specific user
-- This helps when counting chips owned by a user
CREATE INDEX IF NOT EXISTS idx_chips_user_address ON chips(user_address);

-- Composite index on (nft_id, user_address, received) for optimized queries
-- This is the most important index as it covers the exact query pattern used in query_mint:
-- WHERE nft_id = $1 AND LOWER(user_address) = $2 AND received = true
CREATE INDEX IF NOT EXISTS idx_chips_nft_user_received 
ON chips(nft_id, user_address, received);

-- Index on received flag for filtering
-- Useful for queries that filter by received status
CREATE INDEX IF NOT EXISTS idx_chips_received ON chips(received);

-- Add comments to document the purpose of these indexes
COMMENT ON INDEX idx_chips_nft_id IS 'Fast lookup of chips by NFT ID';
COMMENT ON INDEX idx_chips_user_address IS 'Fast lookup of chips by user address';
COMMENT ON INDEX idx_chips_nft_user_received IS 'Composite index for mint eligibility queries (nft_id, user_address, received)';
COMMENT ON INDEX idx_chips_received IS 'Filter chips by received status';

-- Performance statistics (optional, for monitoring)
-- Run ANALYZE to update query planner statistics
ANALYZE chips;





