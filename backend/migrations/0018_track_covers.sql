-- `cover_of_artist_id` — для треков с `upload_kind='cover'`: указывает на
-- оригинального артиста, которого скаверил uploader. Primary_artist_id у
-- кавера ОСТАЁТСЯ NULL (uploader не равен оригиналу), но мы знаем чей это
-- кавер — страница артиста показывает "covers by other artists" одним
-- запросом.

ALTER TABLE tracks ADD COLUMN cover_of_artist_id uuid REFERENCES artists(id);
CREATE INDEX tracks_cover_of_artist_idx
    ON tracks (cover_of_artist_id)
    WHERE cover_of_artist_id IS NOT NULL;
