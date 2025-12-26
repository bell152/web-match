-- Add file_name field to nfts table
-- This field stores the original image file name for each NFT

ALTER TABLE nfts 
ADD COLUMN IF NOT EXISTS file_name VARCHAR(255);

-- Add comment
COMMENT ON COLUMN nfts.file_name IS 'Original image file name (e.g., nft_001.png)';

-- 示例数据（可选，用于测试）
-- UPDATE nfts SET file_name = 'nft_' || LPAD(id::text, 3, '0') || '.png' WHERE file_name IS NULL;



