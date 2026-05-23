use crate::error::AppResult;

use super::types::RecommendResult;
use super::util::value_id_to_string;
use super::RecommendationsService;

impl RecommendationsService {
    pub(crate) async fn take_verified(
        &self,
        items: Vec<RecommendResult>,
        limit: usize,
    ) -> AppResult<Vec<RecommendResult>> {
        let mut out: Vec<RecommendResult> = Vec::new();
        let batch_size = limit.max(8);
        let mut i = 0usize;
        while i < items.len() && out.len() < limit {
            let end = (i + batch_size).min(items.len());
            let slice = &items[i..end];
            let ids: Vec<String> = slice.iter().map(|s| value_id_to_string(&s.id)).collect();
            let missing = self.s3.find_missing(&ids).await?;
            for item in slice {
                if out.len() >= limit {
                    break;
                }
                if !missing.contains(&value_id_to_string(&item.id)) {
                    out.push(item.clone());
                }
            }
            i += batch_size;
        }
        Ok(out)
    }
}
