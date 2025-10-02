pub async fn sync(app: &mut crate::App) -> crate::Result<()> {
    use yansi::Paint;
    
    println!("{}", "Syncing Zig indices...".cyan());
    
    // Force refresh the Zig index from network
    println!("  {} Refreshing Zig index...", "→".blue());
    app.sync_zig_index().await?;
    println!("  {} Zig index synced successfully", "✓".green());
    
    // Force refresh the mirrors list
    println!("  {} Refreshing community mirrors...", "→".blue());
    let mirror_count = app.sync_mirrors().await?;
    println!("  {} Community mirrors synced successfully ({} mirrors)", "✓".green(), mirror_count);
    
    println!("{}", "Sync completed successfully!".green().bold());
    Ok(())
}
