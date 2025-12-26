-- Add token_url field to nfts table
-- This field stores the NFT token metadata URL returned from mint event

ALTER TABLE nfts 
ADD COLUMN IF NOT EXISTS token_url VARCHAR(512);

-- Add comment
COMMENT ON COLUMN nfts.token_url IS 'NFT token metadata URL (e.g., ipfs://QmXxx or https://...)';

-- Add index for token_url lookups
CREATE INDEX IF NOT EXISTS idx_nfts_token_url ON nfts(token_url) WHERE token_url IS NOT NULL;

