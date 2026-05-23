# LTR + future graph features — план возврата

В smart-wave рефакторе **полностью снесли** LtR-пайплайн (`backend/src/modules/ltr/`,
`worker/src/handlers/ltr.py`, NATS-стримы `train.ltr.*` / `ai.rpc.ltr_score`).
Не потому, что LtR-идея плохая — а потому что прежняя реализация была
завязана на `user_taste_*` Qdrant-коллекции и потеряла потребителя инференса
ещё на этапе сноса `wave_fusion`. Зомби-обучение модели, которую никто не
вызывает, мы держать не хотим.

Этот документ — план, как поднять LtR обратно, когда понадобится. Опирается
на новые сигналы из `smart_wave/` и **граф артистов**.

## Где лежит источник фичей

Все нужные сырые сигналы уже строятся в `smart_wave/`:

| Сигнал | Источник | Что им питать |
|--------|----------|---------------|
| Свежие лайки юзера | `smart_wave::signals::load_fresh_likes` (`user_likes_tracks ORDER BY created_at DESC, ctid DESC`) | базовый положительный класс |
| Свежие дизы/скипы | `signals::load_dislikes`, `load_recent_skips` | отрицательный класс |
| Сетка артистов 1-/2-hop | `smart_wave::artist_graph::build_artist_affinity` | граф-фичи трека |
| Track-recs по 3 коллекциям | `smart_wave::track_arm::recommend_from_many` | "вкус-кандидат similarity" фича |
| Cursor с feedback-окном | `smart_wave::cursor::WaveCursor` | per-session signal "юзер в негативном настроении" |

## Целевой набор фичей

`features: Vec<f32>` длины 8 (формат сохранён ради совместимости с
`rec_impressions.features` jsonb, чтобы старая аналитика читалась как было).

| idx | Имя | Описание | Откуда брать |
|-----|-----|----------|--------------|
| 0 | `collab_sim` | cosine(user_collab, track_collab) | уже считается в search.rs и enrichment.rs |
| 1 | `track_arm_score` | z-score из `track_arm` (микс mert+clap+lyrics) | `smart_wave::track_arm::TrackArmCandidate::score` |
| 2 | `artist_affinity` | вес из `artist_graph` для primary_artist трека | `artist_graph::ArtistAffinity::weight` |
| 3 | `artist_hops` | 0/1/2 — насколько далеко артист от seed-вкуса | `artist_graph::ArtistAffinity::hops` |
| 4 | `log_plays` | ln(1 + play_count) / 16 | уже есть в enrichment.rs |
| 5 | `lang_match` | 1/0 — язык трека ∈ языков юзера | уже есть в enrichment.rs |
| 6 | `recency_age_days` | (NOW - indexed_at) в днях, log-normalized | `indexed_tracks.indexed_at` |
| 7 | `neg_session_rate` | доля дизов в текущем окне cursor'а | `WaveCursor::neg_rate()` |

Фичи 6 и 7 — новые. 0/4/5 переиспользуются. 1/2/3 — самое ценное, они и
объясняют, почему именно volna такой формы предлагает трек.

## Что переделать в коде, когда будем включать обратно

1. **Сборщик данных**. Воссоздать `LtrTrainerService` (можно из git history
   `aec0ac4..HEAD`). Заменить старую логику build_user_examples — теперь она
   тянула фичи 1..3 из `user_taste_*` (которых нет). Вместо этого:
   - Тянуть кандидатов из `rec_impressions` (label по тому, был ли в окне
     `[shown_at, +4h]` like/full_play vs skip/dislike — это уже было в
     `build_two_tower_dataset`, можно скопировать).
   - Для каждого (user, track) пересчитать фичи 0..7 как описано выше.
     Артист-фичи стоят дорого: построить `artist_graph` раз на юзера,
     закешировать на время сборки датасета.
2. **NATS-каналы**. Вернуть `TRAIN_LTR` стрим и `AI_LTR_SCORE` subject в
   `backend/src/bus/subjects.rs` + `worker/src/subjects.py`.
3. **Worker-handler**. `worker/src/handlers/ltr.py` — gradient boosted ranker
   (LightGBM подойдёт). На вход — батчи `[group_id, label, features]`,
   на выход — модель сохраняется через безопасный сериализатор (joblib без
   custom-классов или native LightGBM-формат `booster.save_model(...)`).
4. **Inference**. `LtrService::score(features) -> Option<Vec<f32>>` — посылает
   batch на `ai.rpc.ltr_score`, ловит ответ.
5. **Вызов в smart_wave**. В `smart_wave::mod::build` после `blender::blend`
   и `pick_with_cap` — отдельный шаг `LtrService::rerank(items)`, который
   построит фичи (см. таблицу) и применит модель. Не на весь pool, а на топ-N
   (например, 60) — иначе latency.
6. **Cursor**. В `WaveCursor` уже есть `neg_flags` — отдать его в фичу 7.
   Дополнительно положить туда `artist_in_window_count` для anti-spam-фичи
   (не путать с capping — это уже сделано в `pick_with_cap`).

## Чего НЕ делать

- Не возвращать `user_taste_*` коллекции в Qdrant. Они оказались плохой
  абстракцией (одна EMA-точка не описывает многомодальный вкус). Если нужен
  per-user audio centroid — считай на лету как `taste_modes::build_taste_modes`
  по последним лайкам, без записи в Qdrant.
- Не пытаться оживить `two_tower` или `sequential` модели. Первая
  дублировала бы LtR, вторая — track-arm. Их инфраструктура снесена там же,
  где user_taste.

## Когда это нужно

Если smart-wave с фиксированными blender-весами окажется недостаточным.
Признаки:
- Заметно различающийся CTR между близкими кластерами (`for_you` vs `same_vibe`),
  где простая логика весов не справляется.
- Стабильные жалобы юзеров вида "много дизов на ии-реки" даже после adapt'а.
- Есть отдел DS, готовый смотреть feature importance и тюнить.

До этого — лишний LtR-шаг прибавит латентности и сложности без понятного
выигрыша.
