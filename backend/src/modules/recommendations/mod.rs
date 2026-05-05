pub mod handlers;
pub mod s3_verifier;
pub mod service;

pub use handlers::router;
pub use s3_verifier::S3VerifierService;
pub use service::RecommendationsService;
