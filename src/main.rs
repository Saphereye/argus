use clap::{Parser, Subcommand};
use reqwest::Client;
use spinners::{Spinner, Spinners};
use std::{env, process::Command as StdCommand, time::Duration};
use tokio::{process::Child, process::Command as TokioCommand, task, time::sleep};

#[derive(Parser)]
#[command(
    name = "Process Monitor",
    about = "Monitor processes by PID, name, or execute commands."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Monitor a process by PID
    Pid { pid: u32 },
    /// Monitor a process by name
    Name { process_name: String },
    /// Execute a command and monitor it
    Exec { command: String },
}

async fn send_telegram_message(bot_token: &str, chat_id: &str, message: &str) {
    let url = format!("https://api.telegram.org/bot{}/sendMessage", bot_token);
    let client = Client::new();
    if let Err(e) = client
        .post(&url)
        .form(&[("chat_id", chat_id), ("text", message)])
        .send()
        .await
    {
        eprintln!("Failed to send Telegram message: {}", e);
    }
}

async fn monitor_process(mut child: Child) {
    match child.wait().await {
        Ok(status) => {
            if status.success() {
                println!("Process finished successfully.");
            } else {
                eprintln!("Process finished with an error.");
            }
        }
        Err(e) => eprintln!("Error waiting for process to finish: {}", e),
    }
}

async fn monitor_process_by_pid(pid: u32, is_silent: Option<bool>) {
    let wait_time = Duration::from_secs(1);
    let is_silent = is_silent.unwrap_or(false);
    let mut sp = if is_silent {
        None
    } else {
        Some(Spinner::new(Spinners::Moon, format!("Monitoring PID: {}", pid)))
    };

    loop {
        let status = StdCommand::new("ps")
            .arg("-p")
            .arg(pid.to_string())
            .output();
        match status {
            Ok(output) if !output.stdout.is_empty() => {
                sleep(wait_time).await;
            }
            _ => {
                if let Some(ref mut spinner) = sp {
                    spinner.stop();
                }
                println!("Process with PID {} has terminated.", pid);
                break;
            }
        }
    }
}

async fn monitor_process_by_name(process_name: &str) {
    let wait_time = Duration::from_secs(1);
    let mut sp = Spinner::new(
        Spinners::Moon,
        format!("Monitoring processes named: {}", process_name),
    );
    loop {
        let status = StdCommand::new("pgrep").arg(process_name).output();
        match status {
            Ok(output) => {
                let output_str = String::from_utf8_lossy(&output.stdout);
                let pids: Vec<u32> = output_str
                    .lines()
                    .filter_map(|line| line.trim().parse::<u32>().ok())
                    .collect();
                if pids.is_empty() {
                    sp.stop();
                    println!("\nAll processes named '{}' have terminated.", process_name);
                    break;
                } else {
                    for pid in pids {
                        let _ = task::spawn(monitor_process_by_pid(pid, Some(true)));
                    }
                }
            }
            Err(_) => {
                sp.stop();
                println!("\nError retrieving process list.");
                break;
            }
        }
        sleep(wait_time).await;
    }
}

async fn execute_and_monitor_command(command: &str) -> std::io::Result<Child> {
    let child = TokioCommand::new("sh")
        .arg("-c")
        .arg(command)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    println!("Started command '{}', PID: {:?}", command, child.id());
    Ok(child)
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let bot_token = env::var("BOT_TOKEN").expect("BOT_TOKEN not set");
    let chat_id = env::var("CHAT_ID").expect("CHAT_ID not set");

    match cli.command {
        Commands::Pid { pid } => {
            send_telegram_message(
                &bot_token,
                &chat_id,
                &format!("Starting to monitor PID: {}", pid),
            )
            .await;
            monitor_process_by_pid(pid, None).await;
            send_telegram_message(
                &bot_token,
                &chat_id,
                &format!("Process {} has finished.", pid),
            )
            .await;
        }
        Commands::Name { process_name } => {
            send_telegram_message(
                &bot_token,
                &chat_id,
                &format!("Monitoring processes named: {}", process_name),
            )
            .await;
            monitor_process_by_name(&process_name).await;
            send_telegram_message(
                &bot_token,
                &chat_id,
                &format!("Processes '{}' have finished.", process_name),
            )
            .await;
        }
        Commands::Exec { command } => {
            send_telegram_message(
                &bot_token,
                &chat_id,
                &format!("Starting command: '{}'", command),
            )
            .await;
            match execute_and_monitor_command(&command).await {
                Ok(child) => {
                    let monitor_task = task::spawn(monitor_process(child));
                    monitor_task.await.unwrap();
                    send_telegram_message(
                        &bot_token,
                        &chat_id,
                        &format!("Command '{}' has finished.", command),
                    )
                    .await;
                }
                Err(e) => {
                    eprintln!("Failed to execute command: {}", e);
                }
            }
        }
    }
}
