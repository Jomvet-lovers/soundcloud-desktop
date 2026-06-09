WITH agg AS (SELECT ta.artist_id, COUNT(*) ::real AS score
             FROM user_events ue
                      JOIN tracks it ON it.sc_track_id = ue.sc_track_id
                      JOIN track_artists ta ON ta.track_id = it.id
             WHERE ue.created_at > now() - interval '30 days'
GROUP BY ta.artist_id
    )
UPDATE artists a
SET interest_score = agg.score FROM agg
WHERE a.id = agg.artist_id AND a.interest_score IS DISTINCT
FROM agg.score
