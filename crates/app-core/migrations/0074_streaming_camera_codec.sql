-- The codec the edge probed off a streaming camera's own SDP ("H264"/"H265"), reported
-- alongside its resolved-stream IP (#144 follow-up: the live MSE mux branch turned out to
-- silently never work for H265 cameras on some backends — surfacing the codec on the Devices
-- page makes that visible instead of just "no video"). NULL until an updated edge reports it.
ALTER TABLE streaming_cameras ADD COLUMN IF NOT EXISTS codec text;
