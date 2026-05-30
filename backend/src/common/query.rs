/// Парсит CSV языков из query-параметра (`?languages=ru,en`): тримит, отбрасывает
/// пустые; пустой/отсутствующий список → None (без языкового фильтра).
pub fn parse_languages(raw: Option<&str>) -> Option<Vec<String>> {
    let s = raw?;
    let v: Vec<String> = s
        .split(',')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(String::from)
        .collect();
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}
