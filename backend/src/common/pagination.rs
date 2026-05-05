use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct PaginationQuery {
    #[serde(default, deserialize_with = "parse_opt_int")]
    pub page: Option<i64>,
    #[serde(default, deserialize_with = "parse_opt_int")]
    pub limit: Option<i64>,
}

impl PaginationQuery {
    pub fn page(&self) -> i64 {
        self.page.unwrap_or(0).max(0)
    }

    pub fn limit(&self) -> i64 {
        self.limit.unwrap_or(30).clamp(1, 200)
    }

    pub fn resolved(&self) -> (i64, i64) {
        (self.page(), self.limit())
    }
}

fn parse_opt_int<'de, D>(d: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let raw: Option<String> = Option::deserialize(d)?;
    match raw {
        None => Ok(None),
        Some(s) if s.is_empty() => Ok(None),
        Some(s) => s.parse::<i64>().map(Some).map_err(D::Error::custom),
    }
}
