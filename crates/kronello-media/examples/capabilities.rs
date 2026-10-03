fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = kronello_media::MediaRuntime::load()?;
    if std::env::args().any(|arg| arg == "--verify-distribution") {
        runtime.capabilities().verify_distribution()?;
    }
    println!("{}", serde_json::to_string_pretty(runtime.capabilities())?);
    Ok(())
}
