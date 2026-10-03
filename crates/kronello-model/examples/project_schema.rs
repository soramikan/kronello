fn main() {
    println!(
        "{}",
        serde_json::to_string_pretty(&kronello_model::project_json_schema()).unwrap()
    );
}
