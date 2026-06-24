-- Add a human-friendly 3-word name to each recording (generated on the edge at stop() time).
ALTER TABLE recordings ADD COLUMN friendly_name TEXT NOT NULL DEFAULT '';
