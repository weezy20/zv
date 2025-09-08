use crate::App;
use crate::Template;
use color_eyre::Result;
use yansi::Paint;

pub(crate) fn init_project(template: Template, app: &App) -> Result<()> {
    let template_result = template.execute(app)?;
    let project_name = template_result.project_name;

    if let Some(msg) = &template_result.pre_exec_msg {
        println!("{}", Paint::new(msg).italic().dim());
    }
    for status in &template_result.file_statuses {
        use crate::FileStatus;
        match status {
            FileStatus::Created(path) => {
                println!(
                    "{} {}",
                    Paint::new("  Created").dim().italic(),
                    Paint::new(path.display()).bright_blue()
                );
            }
            FileStatus::Preserved(path) => {
                println!(
                    "{} {}",
                    Paint::new("  Preserving").dim().italic(),
                    Paint::new(path.display()).bright_blue()
                );
            }
        }
    }
    /* Post template action */
    match project_name {
        Some(name) => {
            println!(
                "{} {}",
                Paint::new("✔ Project initialized at").bright_green(),
                Paint::new(&name).bold().bright_blue()
            );

            println!("\n{}", Paint::new("→ Next steps:").bold().underline());

            println!(
                "  {} {}",
                Paint::new("cd").bright_white().bold(),
                Paint::new(name).bright_blue().bold()
            );

            println!("  {}", Paint::new("zig build run").italic().bright_white());
        }
        None => {
            println!(
                "{}",
                Paint::new("✔ Project initialized at current working directory.")
                    .bright_green()
                    .bold()
            );

            println!("\n{}", Paint::new("→ Next steps:").bold().underline());

            println!("  {}", Paint::new("zig build run").italic().bright_white());
        }
    }
    Ok(())
}
