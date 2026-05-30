//! `/search/db/*` — поиск внутри нашей базы (tracks/playlists/users/artists/
//! albums). Альтернатива дорогому fan-out'у в SC API: значительно быстрее, но
//! ограничен тем, что мы уже зеркалили.
//!
//! Под high-load прод: каждая выдача — короткая, ограниченная транзакция с
//! `statement_timeout`, результат кладётся в Redis на TTL_SEARCH, чтобы не
//! долбить trgm-индекс одинаковыми запросами с десятка клиентов.

pub mod handlers;
pub mod repository;
pub mod service;
pub mod vibe;

pub use handlers::router;
pub use service::SearchService;
pub use vibe::VibeSearchService;
