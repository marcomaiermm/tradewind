fn main() -> anyhow::Result<()> {
    if std::env::args().any(|arg| arg == "--version" || arg == "-V") {
        println!("tradewind {}", tradewind::APP_VERSION);
        return Ok(());
    }
    if let Some(path) = std::env::args().skip_while(|s| s != "--inspect").nth(1) {
        let runtime = tokio::runtime::Runtime::new()?;
        let (coordinator, mut settings) = tradewind::application::Coordinator::new()?;
        settings.wow_path = path.into();
        // Inspection is read-only, so the root is overridden without persisting settings.
        let outcome = runtime.block_on(coordinator.inspect_at(settings.wow_path));
        match outcome.result {
            Ok(_) => println!(
                "Game versions: {}\nTSM accounts: {}\nAddons: {}",
                outcome.snapshot.installation.games.len(),
                outcome.snapshot.installation.accounts.len(),
                outcome.snapshot.installation.addons.len()
            ),
            Err(error) => anyhow::bail!(error),
        }
        return Ok(());
    }
    tradewind::ui::run(tradewind::ui::Args::parse(std::env::args()))?;
    Ok(())
}
