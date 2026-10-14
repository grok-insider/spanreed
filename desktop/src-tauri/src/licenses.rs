#[tauri::command]
pub fn licenses() -> String {
    [
        include_str!("../../../LICENSE"),
        "\n\n",
        include_str!(concat!(env!("OUT_DIR"), "/native-notices.txt")),
        "\n\n",
        include_str!(concat!(env!("OUT_DIR"), "/frontend-notices.txt")),
    ]
    .concat()
}
