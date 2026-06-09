SELECT calibrated
FROM enrich_calibration
WHERE source = $1
  AND raw_bin_low <= $2
  AND $2 < raw_bin_high
ORDER BY raw_bin_low DESC LIMIT 1
