-- Add coordinate and file name columns to chips table
-- These fields are used for image tile positioning and file references

ALTER TABLE chips
ADD COLUMN IF NOT EXISTS x INTEGER,
ADD COLUMN IF NOT EXISTS y INTEGER,
ADD COLUMN IF NOT EXISTS w INTEGER,
ADD COLUMN IF NOT EXISTS h INTEGER,
ADD COLUMN IF NOT EXISTS file_name VARCHAR(255);

-- Add comments to document the fields
COMMENT ON COLUMN chips.x IS 'X coordinate of the chip in the original image';
COMMENT ON COLUMN chips.y IS 'Y coordinate of the chip in the original image';
COMMENT ON COLUMN chips.w IS 'Width of the chip';
COMMENT ON COLUMN chips.h IS 'Height of the chip';
COMMENT ON COLUMN chips.file_name IS 'Tile image file name (e.g., "specimen/specimen_21.png")';

