use uuid::Uuid;

pub fn parse_uuid(value: &str) -> Option<Uuid> {
    Uuid::parse_str(value).ok()
}
