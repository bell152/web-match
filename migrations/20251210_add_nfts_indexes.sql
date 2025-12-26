-- Add performance indexes to nfts table for query_mint optimization
-- This migration adds indexes to improve the performance of NFT-related queries

-- Index on user_address for fast lookups of NFTs owned by a specific user
-- This is crucial for the first step of query_mint
CREATE INDEX IF NOT EXISTS idx_nfts_user_address ON nfts(user_address);

-- Composite index on (user_address, received) for optimized queries
-- This covers the exact query pattern: WHERE LOWER(user_address) = $1 AND received = true
CREATE INDEX IF NOT EXISTS idx_nfts_user_received 
ON nfts(user_address, received);

-- Index on received flag
CREATE INDEX IF NOT EXISTS idx_nfts_received ON nfts(received);

-- Add comments to document the purpose of these indexes
COMMENT ON INDEX idx_nfts_user_address IS 'Fast lookup of NFTs by user address';
COMMENT ON INDEX idx_nfts_user_received IS 'Composite index for finding received NFTs by user (user_address, received)';
COMMENT ON INDEX idx_nfts_received IS 'Filter NFTs by received status';

-- Performance statistics
ANALYZE nfts;





