fn main() {
    println!(
        "{}",
        serde_json::to_string_pretty(&kronello_service::api_json_schema()).unwrap()
    );
}
