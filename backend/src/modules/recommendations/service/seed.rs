use crate::qdrant::collections;

use super::types::SeedVectors;
use super::RecommendationsService;

impl RecommendationsService {
    pub(crate) async fn load_track_vectors(&self, track_id: u64) -> SeedVectors {
        let collab_fut = async { self.collab.get_track_vector(track_id).await };
        let m_fut = self.retrieve_vector(collections::TRACKS_MERT, track_id);
        let c_fut = self.retrieve_vector(collections::TRACKS_CLAP, track_id);
        let l_fut = self.retrieve_vector(collections::TRACKS_LYRICS, track_id);
        let (collab, mert, clap, lyrics) = tokio::join!(collab_fut, m_fut, c_fut, l_fut);
        SeedVectors {
            collab,
            mert,
            clap,
            lyrics,
        }
    }
}
