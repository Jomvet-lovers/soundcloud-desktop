-- Ко-лайк рёбра «фанаты артиста A лайкают и B»: Ochiai-близость по пересечению
-- лайкеров. Пересчитывается кроном recommendations::smart_wave::colike,
-- читается сеткой волны вместе с коллаб-рёбрами artist_coplay.
CREATE TABLE IF NOT EXISTS artist_colike (
    a_id       uuid        NOT NULL REFERENCES artists (id) ON DELETE CASCADE,
    b_id       uuid        NOT NULL REFERENCES artists (id) ON DELETE CASCADE,
    co         integer     NOT NULL,
    w          real        NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (a_id, b_id),
    CHECK (a_id < b_id)
);
CREATE INDEX IF NOT EXISTS artist_colike_a_w_idx ON artist_colike (a_id, w DESC);
CREATE INDEX IF NOT EXISTS artist_colike_b_w_idx ON artist_colike (b_id, w DESC);
